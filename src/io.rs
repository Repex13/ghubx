//! Device IO thread: sole owner of the HID handles. Executes DevCmds,
//! forwards 0x8110 button bitmaps to the engine, publishes DeviceState
//! snapshots, and reconnects after unplug/replug.

use crate::device::{DeviceState, G502, MODE_HOST, MODE_ONBOARD};
use crate::engine::{DevCmd, EngineMsg, UiEvent};
use crate::hidpp::Result;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use hidapi::HidApi;
use std::time::{Duration, Instant};

const IDENTITY_MAP: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0];

pub fn run(cmd_rx: Receiver<DevCmd>, eng: Sender<EngineMsg>, ui: Sender<UiEvent>) {
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
        'conn: loop {
            loop {
                match cmd_rx.try_recv() {
                    Ok(DevCmd::Quit) | Err(TryRecvError::Disconnected) => return,
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
            for (z, led) in ap.leds.iter().enumerate() {
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
        DevCmd::Quit => {}
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
    for z in 0..2u8 {
        if let Ok(l) = dev.get_zone_effect(z) {
            st.zone_leds[z as usize] = l;
        }
    }
}
