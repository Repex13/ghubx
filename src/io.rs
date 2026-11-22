//! Device IO thread: sole owner of the HID handles. Executes DevCmds,
//! forwards 0x8110 button bitmaps to the engine, publishes DeviceState
//! snapshots, and reconnects after unplug/replug.

use crate::device::{DeviceState, G502, MODE_HOST, MODE_ONBOARD};
use crate::engine::{DevCmd, EngineMsg, UiEvent};
use crate::hidpp::Result;
use crate::profile::{OnboardProfile, OnboardSlot, parse_directory};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use hidapi::HidApi;
use std::time::{Duration, Instant};

const IDENTITY_MAP: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0];

/// Read-only walk over the mouse's on-board profile memory, one sector per IO pass.
struct OnboardJob {
    dir: Option<Vec<(u16, bool)>>,
    slots: Vec<OnboardSlot>,
}

impl OnboardJob {
    /// Do one sector; returns the finished list when done.
    fn step(&mut self, dev: &G502) -> Option<Vec<OnboardSlot>> {
        let Some(dir) = &self.dir else {
            self.dir = Some(dev.read_sector(0).map(|s| parse_directory(&s)).unwrap_or_default());
            return self.dir.as_ref().is_some_and(|d| d.is_empty()).then(Vec::new);
        };
        let (sector, enabled) = dir[self.slots.len()];
        let profile = dev.read_sector(sector).ok().and_then(|s| OnboardProfile::parse(&s));
        self.slots.push(OnboardSlot { sector, enabled, profile });
        (self.slots.len() == dir.len()).then(|| std::mem::take(&mut self.slots))
    }
}

pub fn run(cmd_rx: Receiver<DevCmd>, eng: Sender<EngineMsg>, ui: Sender<UiEvent>, mailbox: crate::fx::Mailbox) {
    let mut api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            let _ = ui.send(UiEvent::Log(format!("HID init failed: {e}")));
            return;
        }
    };
    let mut announced = false;
    loop {
        let mut dev = loop {
            match G502::connect(&api) {
                Ok(d) => break d,
                Err(e) => {
                    if !announced {
                        announced = true;
                        let _ = ui.send(UiEvent::Device(DeviceState::default()));
                        let _ = ui.send(UiEvent::Log(format!("Waiting for G502 HERO: {e}")));
                    }
                    match cmd_rx.recv_timeout(Duration::from_millis(1500)) {
                        Ok(DevCmd::Quit) | Err(RecvTimeoutError::Disconnected) => return,
                        _ => {}
                    }
                    let _ = api.refresh_devices();
                }
            }
        };
        announced = false;
        let mut st = DeviceState { connected: true, info: dev.info.clone(), ..Default::default() };
        refresh(&dev, &mut st);
        let _ = ui.send(UiEvent::Device(st.clone()));
        let _ = eng.send(EngineMsg::Connected(true));

        let mut last = Instant::now();
        let mut frames = Vec::new();
        let mut onboard_job: Option<OnboardJob> = None;
        'conn: loop {
            loop {
                match cmd_rx.try_recv() {
                    Ok(DevCmd::Quit) | Err(TryRecvError::Disconnected) => return,
                    Ok(DevCmd::ReadOnboard) => {
                        if crate::win::ghub_running() {
                            let _ = ui.send(UiEvent::Log("On-board profiles not read: G HUB is running".into()));
                            let _ = ui.send(UiEvent::Onboard(Vec::new()));
                        } else {
                            onboard_job = Some(OnboardJob { dir: None, slots: Vec::new() });
                        }
                    }
                    Ok(DevCmd::WriteOnboard { sectors, active }) => {
                        let mut log = |m: String| {
                            let _ = ui.send(UiEvent::Log(format!("On-board write: {m}")));
                        };
                        let res = crate::onboard_write::write_sectors(&dev, &sectors, active, &mut log);
                        refresh(&dev, &mut st);
                        let _ = ui.send(UiEvent::OnboardWritten(res));
                        let _ = ui.send(UiEvent::Device(st.clone()));
                        onboard_job = Some(OnboardJob { dir: None, slots: Vec::new() });
                        let _ = eng.send(EngineMsg::Reapply);
                    }
                    Ok(DevCmd::RestoreOnboard { slots }) => {
                        let mut log = |m: String| {
                            let _ = ui.send(UiEvent::Log(format!("On-board restore: {m}")));
                        };
                        let res = crate::onboard_write::restore_slots(&dev, &slots, &mut log);
                        refresh(&dev, &mut st);
                        let _ = ui.send(UiEvent::OnboardWritten(res));
                        let _ = ui.send(UiEvent::Device(st.clone()));
                        onboard_job = Some(OnboardJob { dir: None, slots: Vec::new() });
                        let _ = eng.send(EngineMsg::Reapply);
                    }
                    Ok(cmd) => {
                        let take = matches!(cmd, DevCmd::TakeControl(_));
                        let r = exec(&dev, &mut st, cmd);
                        if let Err(e) = &r {
                            let _ = ui.send(UiEvent::Log(format!("Device: {e}")));
                        }
                        if take {
                            let _ = eng.send(EngineMsg::ControlResult(r.is_ok()));
                        }
                        let _ = ui.send(UiEvent::Device(st.clone()));
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
            if st.controlled {
                for (zone, rgb) in mailbox.take().into_iter().enumerate() {
                    if let Some(color) = rgb {
                        let led = crate::profile::Lighting { effect: crate::profile::Effect::Fixed, color, ..Default::default() }.to_led();
                        // RAM only: a host-driven colour must never reach the flash
                        if dev.set_zone_effect(zone as u8, &led, false).is_ok() {
                            st.zone_leds[zone] = led;
                        }
                    }
                }
            }
            if let Some(job) = &mut onboard_job
                && let Some(slots) = job.step(&dev)
            {
                onboard_job = None;
                let _ = ui.send(UiEvent::Onboard(slots));
            }
            frames.clear();
            if dev.poll_events(&mut frames).is_err() {
                break 'conn;
            }
            for f in &frames {
                if let Some(mask) = dev.parse_spy(f) {
                    let _ = eng.send(EngineMsg::Spy(mask));
                }
            }
            // Poll gently while G HUB owns the mouse so we don't race its requests.
            let every = if st.controlled { Duration::from_millis(1000) } else { Duration::from_millis(3000) };
            if last.elapsed() >= every {
                last = Instant::now();
                refresh(&dev, &mut st);
                if st.controlled && st.mode != MODE_HOST {
                    st.controlled = false;
                    let _ = ui.send(UiEvent::Log("Mouse left host mode; taking control again".into()));
                    let _ = eng.send(EngineMsg::ControlLost);
                }
                let _ = ui.send(UiEvent::Device(st.clone()));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = ui.send(UiEvent::Log("G502 HERO disconnected".into()));
        let _ = ui.send(UiEvent::Device(DeviceState::default()));
        let _ = eng.send(EngineMsg::Connected(false));
        let _ = api.refresh_devices();
    }
}

fn exec(dev: &G502, st: &mut DeviceState, cmd: DevCmd) -> Result<()> {
    match cmd {
        DevCmd::TakeControl(ap) => {
            dev.set_mode(MODE_HOST)?;
            dev.spy_set_map(&ap.map)?;
            dev.spy_start()?;
            dev.set_dpi(ap.dpi)?;
            // Report rate is host-mode only; LED writes are best effort.
            let _ = dev.set_rate_ms(ap.rate_ms);
            for (z, led) in ap.leds.iter().enumerate().filter(|_| ap.send_leds) {
                let _ = dev.set_zone_effect(z as u8, led, false);
            }
            st.controlled = true;
            refresh(dev, st);
        }
        DevCmd::Release => {
            let _ = dev.spy_stop();
            let _ = dev.spy_set_map(&IDENTITY_MAP);
            dev.set_mode(MODE_ONBOARD)?;
            st.controlled = false;
            refresh(dev, st);
        }
        DevCmd::Yield => st.controlled = false,
        DevCmd::SetDpiLedAlwaysOn(on) => {
            // stored in the mouse: only write when it really changes
            if st.dpi_led_always_on != Some(on) {
                dev.set_dpi_led_always_on(on)?;
            }
            st.dpi_led_always_on = Some(on);
        }
        DevCmd::DpiLed(level) => dev.set_dpi_led_level(level)?,
        DevCmd::SetStartupEffect(on) => {
            dev.set_startup_effect(on)?;
            st.startup_effect = Some(on);
        }
        DevCmd::SetDpi(d) => {
            dev.set_dpi(d)?;
            st.dpi = d;
        }
        DevCmd::SetRate(ms) => {
            dev.set_rate_ms(ms)?;
            st.rate_ms = ms;
        }
        DevCmd::SetMap(m) => {
            dev.spy_set_map(&m)?;
            st.spy_map = m;
        }
        DevCmd::SetZone { zone, led } => {
            dev.set_zone_effect(zone, &led, false)?;
            if let Some(z) = st.zone_leds.get_mut(zone as usize) {
                *z = led;
            }
        }
        DevCmd::SetOnboardProfile(slot) => dev.set_onboard_profile(slot)?,
        DevCmd::Refresh => refresh(dev, st),
        DevCmd::ReadOnboard | DevCmd::WriteOnboard { .. } | DevCmd::RestoreOnboard { .. } | DevCmd::Quit => {}
    }
    Ok(())
}

fn refresh(dev: &G502, st: &mut DeviceState) {
    if let Ok(m) = dev.get_mode() {
        st.mode = m;
    }
    st.onboard_profile = dev.get_onboard_profile().unwrap_or(0);
    if let Ok((d, def)) = dev.get_dpi() {
        st.dpi = d;
        st.default_dpi = def;
    }
    if let Ok(r) = dev.get_rate_ms() {
        st.rate_ms = r;
    }
    if let Ok(m) = dev.spy_get_map() {
        st.spy_map = m;
    }
    st.dpi_led_always_on = dev.get_dpi_led_always_on().ok();
    st.startup_effect = dev.get_startup_effect().ok();
    for z in 0..2u8 {
        if let Ok(l) = dev.get_zone_effect(z) {
            st.zone_leds[z as usize] = l;
        }
    }
}
