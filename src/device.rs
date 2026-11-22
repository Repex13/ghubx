//! G502 HERO device model — high-level operations over the HID++ core.
//! All calls must run on the single owner thread (reads share the event
//! handle with the event poll).

use crate::hidpp::*;
use hidapi::{HidApi, HidDevice};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const VID: u16 = 0x046D;
pub const PID: u16 = 0xC08B;

/// Onboard mode constants (0x8100 fn1/fn2).
pub const MODE_ONBOARD: u8 = 0x01;
pub const MODE_HOST: u8 = 0x02;

/// LED modes for internal_led[0] (0x8070).
pub const LED_OFF: u8 = 0x00;
pub const LED_FIXED: u8 = 0x01;
pub const LED_CYCLE: u8 = 0x03;
pub const LED_BREATHING: u8 = 0x0A;

/// Number of physical button slots reported by 0x8110 on the G502 HERO
/// (bit n of the spy bitmap = slot n = G(n+1)).
pub const SLOTS: usize = 11;

/// Report intervals in ms supported by 0x8060 (bitmask bit n = n+1 ms).
pub const RATES_MS: [u8; 4] = [1, 2, 4, 8];

pub fn rate_hz(ms: u8) -> u16 {
    if ms == 0 { 0 } else { 1000 / ms as u16 }
}

#[derive(Debug, Clone, Default)]
pub struct DeviceInfo {
    pub name: String,
    /// G HUB style, e.g. "127.3.10".
    pub fw: String,
    pub bootloader: String,
    pub protocol: (u8, u8),
    pub zones: u8,
    pub buttons: u8,
    pub profile_count: u8,
    pub dpi_stages: u8,
    pub sector_size: u16,
    pub dpi_min: u16,
    pub dpi_max: u16,
    pub dpi_step: u16,
    pub features: Vec<(u8, u16, u8)>, // (index, id, version)
}

#[derive(Debug, Clone, Default)]
pub struct DeviceState {
    pub connected: bool,
    pub info: DeviceInfo,
    pub mode: u8,
    pub onboard_profile: u8,
    pub dpi: u16,
    pub default_dpi: u16,
    pub rate_ms: u8,
    pub spy_map: [u8; 16],
    /// True while GHUB-X holds host mode with the spy running.
    pub controlled: bool,
    pub ghub_running: bool,
    pub zone_leds: [[u8; 11]; 2],
}

/// Owns both HID handles. Lives on the IO thread.
pub struct G502 {
    w: HidDevice,
    r: HidDevice,
    stray: Arc<Mutex<VecDeque<Vec<u8>>>>,
    idx: std::collections::HashMap<u16, u8>,
    pub info: DeviceInfo,
}

impl G502 {
    pub fn connect(api: &HidApi) -> Result<Self> {
        let (mut ctrl, mut evt) = (None, None);
        for d in api.device_list() {
            if d.vendor_id() == VID && d.product_id() == PID && d.usage_page() >= 0xFF00 {
                match d.usage() {
                    1 => ctrl = Some(d.path().to_owned()),
                    2 => evt = Some(d.path().to_owned()),
                    _ => {}
                }
            }
        }
        let w = api.open_path(&ctrl.ok_or(HidppError::Other("G502 HERO not found".into()))?)?;
        let r = api.open_path(&evt.ok_or(HidppError::Other("G502 HERO event channel not found".into()))?)?;
        let _ = w.set_blocking_mode(false);
        let _ = r.set_blocking_mode(false);

        let mut dev = Self {
            w,
            r,
            stray: Default::default(),
            idx: Default::default(),
            info: DeviceInfo::default(),
        };
        let (proto, feats) = {
            let pp = dev.pp();
            (pp.protocol_version().unwrap_or((0, 0)), enumerate_features(&pp)?)
        };
        dev.info.protocol = proto;
        dev.idx = feats.iter().map(|f| (f.id, f.index)).collect();
        dev.info.features = feats.iter().map(|f| (f.index, f.id, f.version)).collect();
        dev.info.name = dev.device_name().unwrap_or_else(|_| "G502 HERO".into());
        let (fw, boot) = dev.firmware().unwrap_or_default();
        dev.info.fw = fw;
        dev.info.bootloader = boot;
        if let Ok(d) = dev.request(FEAT_ONBOARD_PROFILES, 0, &[]) {
            dev.info.profile_count = d[3];
            dev.info.dpi_stages = d[4];
            dev.info.buttons = d[5];
            dev.info.sector_size = u16::from_be_bytes([d[7], d[8]]);
        }
        if let Ok(p) = dev.request(FEAT_ADJUSTABLE_DPI, 1, &[0]) {
            // [sensor, min_hi, min_lo, 0xE0|step_hi?, step_lo, max_hi, max_lo]
            dev.info.dpi_min = u16::from_be_bytes([p[1], p[2]]);
            dev.info.dpi_step = u16::from_be_bytes([p[3] & 0x1F, p[4]]);
            dev.info.dpi_max = u16::from_be_bytes([p[5], p[6]]);
        }
        if dev.info.dpi_max == 0 {
            (dev.info.dpi_min, dev.info.dpi_step, dev.info.dpi_max) = (100, 50, 25600);
        }
        dev.info.zones = dev.zone_count().unwrap_or(2);
        Ok(dev)
    }

    fn pp(&self) -> HidppDevice<'_> {
        HidppDevice::with_stray(&self.w, &self.r, DEV_BROADCAST, self.stray.clone())
    }

    fn fi(&self, id: u16) -> Result<u8> {
        self.idx.get(&id).copied().ok_or(HidppError::FeatureNotSupported(id))
    }

    pub fn request(&self, feat: u16, func: u8, params: &[u8]) -> Result<Vec<u8>> {
        let idx = self.fi(feat)?;
        self.pp().request(idx, func, params)
    }

    /// Drain frames seen mid-request plus whatever is queued on the handle.
    pub fn poll_events(&mut self, out: &mut Vec<Vec<u8>>) -> Result<()> {
        out.extend(self.pp().drain_stray());
        let mut b = [0u8; 64];
        loop {
            let n = self.r.read_timeout(&mut b, 0)?;
            if n == 0 {
                return Ok(());
            }
            out.push(b[..n].to_vec());
        }
    }

    /// Decode a 0x8110 button event: `[0x11, 0xFF, spy_idx, fn0|swid0, mask_hi, mask_lo, ..]`.
    /// Returns the bitmap of currently held slots.
    pub fn parse_spy(&self, msg: &[u8]) -> Option<u16> {
        let spy = *self.idx.get(&FEAT_MOUSE_BUTTON_SPY)?;
        decode_spy(spy, msg)
    }

    // ---- identity ----

    fn device_name(&self) -> Result<String> {
        let idx = self.fi(FEAT_DEVICE_NAME)?;
        let pp = self.pp();
        let len = *pp.request(idx, 0, &[])?.first().unwrap_or(&0) as usize;
        let mut s = Vec::new();
        while s.len() < len {
            let chunk = pp.request(idx, 1, &[s.len() as u8])?;
            let take = (len - s.len()).min(chunk.len());
            if take == 0 {
                break;
            }
            s.extend_from_slice(&chunk[..take]);
        }
        Ok(String::from_utf8_lossy(&s).trim_end_matches('\0').to_string())
    }

    /// (main firmware, bootloader) in G HUB's display format.
    fn firmware(&self) -> Result<(String, String)> {
        let idx = self.fi(FEAT_DEVICE_FW_VERSION)?;
        let pp = self.pp();
        let count = *pp.request(idx, 0, &[])?.first().unwrap_or(&0);
        let (mut fw, mut boot) = (String::new(), String::new());
        for i in 0..count {
            if let Ok(v) = pp.request(idx, 1, &[i]) {
                match v.first() {
                    Some(0) => fw = format_fw(&v),
                    Some(1) => boot = format_fw(&v),
                    _ => {}
                }
            }
        }
        Ok((fw, boot))
    }

    // ---- DPI (0x2201) ----

    /// (current dpi, default dpi)
    pub fn get_dpi(&self) -> Result<(u16, u16)> {
        let p = self.request(FEAT_ADJUSTABLE_DPI, 2, &[0])?;
        Ok((
            u16::from_be_bytes([p[1], p[2]]),
            u16::from_be_bytes([p[3], p[4]]),
        ))
    }

    pub fn set_dpi(&self, dpi: u16) -> Result<()> {
        self.request(FEAT_ADJUSTABLE_DPI, 3, &[0, (dpi >> 8) as u8, dpi as u8])?;
        Ok(())
    }

    // ---- report rate (0x8060): values are report intervals in ms ----

    pub fn get_rate_ms(&self) -> Result<u8> {
        Ok(*self.request(FEAT_ADJUSTABLE_REPORT_RATE, 1, &[])?.first().unwrap_or(&1))
    }

    /// Only accepted in host mode.
    pub fn set_rate_ms(&self, ms: u8) -> Result<()> {
        self.request(FEAT_ADJUSTABLE_REPORT_RATE, 2, &[ms])?;
        Ok(())
    }

    // ---- onboard profiles (0x8100) ----

    pub fn get_mode(&self) -> Result<u8> {
        Ok(*self.request(FEAT_ONBOARD_PROFILES, 2, &[])?.first().unwrap_or(&0))
    }

    pub fn set_mode(&self, mode: u8) -> Result<()> {
        self.request(FEAT_ONBOARD_PROFILES, 1, &[mode])?;
        Ok(())
    }

    /// Active onboard slot (1-based); 0 while in host mode.
    pub fn get_onboard_profile(&self) -> Result<u8> {
        Ok(self.request(FEAT_ONBOARD_PROFILES, 4, &[])?.get(1).copied().unwrap_or(0))
    }

    pub fn set_onboard_profile(&self, slot: u8) -> Result<()> {
        self.request(FEAT_ONBOARD_PROFILES, 3, &[0, slot])?;
        Ok(())
    }

    /// fn5 memoryRead(sector u16, offset u16) -> 16 bytes.
    pub fn mem_read(&self, sector: u16, offset: u16) -> Result<[u8; 16]> {
        let p = self.request(FEAT_ONBOARD_PROFILES, 5, &[
            (sector >> 8) as u8, sector as u8, (offset >> 8) as u8, offset as u8,
        ])?;
        let mut out = [0u8; 16];
        let n = p.len().min(16);
        out[..n].copy_from_slice(&p[..n]);
        Ok(out)
    }

    pub fn read_sector(&self, sector: u16) -> Result<[u8; 256]> {
        let mut out = [0u8; 256];
        for off in (0..256u16).step_by(16) {
            out[off as usize..off as usize + 16].copy_from_slice(&self.mem_read(sector, off)?);
        }
        Ok(out)
    }

    /// libratbag `hidpp20_onboard_profiles_write_sector`: fn6 addrWrite
    /// [sector BE][sub-address BE = 0][byte count BE], 16-byte fn7 writes, fn8 end.
    /// The caller is responsible for a valid CRC in the last two bytes.
    pub fn write_sector(&self, sector: u16, data: &[u8; 256]) -> Result<()> {
        self.request(FEAT_ONBOARD_PROFILES, 6, &[
            (sector >> 8) as u8, sector as u8, 0, 0, 0x01, 0x00,
        ])?;
        for chunk in data.chunks(16) {
            self.request(FEAT_ONBOARD_PROFILES, 7, chunk)?;
        }
        self.request(FEAT_ONBOARD_PROFILES, 8, &[])?;
        Ok(())
    }

    // ---- color LED effects (0x8070) ----

    fn zone_count(&self) -> Result<u8> {
        Ok(*self.request(FEAT_COLOR_LED_EFFECTS, 0, &[])?.first().unwrap_or(&0))
    }

    /// fn14 (0xE) getZoneEffect -> [zone][internal_led 11B]
    pub fn get_zone_effect(&self, zone: u8) -> Result<[u8; 11]> {
        let p = self.request(FEAT_COLOR_LED_EFFECTS, 0x0E, &[zone])?;
        let mut led = [0u8; 11];
        let n = p.len().saturating_sub(1).min(11);
        led[..n].copy_from_slice(&p[1..1 + n]);
        Ok(led)
    }

    /// fn3 setZoneEffect (long). `persist` also stores it in the device.
    pub fn set_zone_effect(&self, zone: u8, led: &[u8; 11], persist: bool) -> Result<()> {
        let mut p = vec![zone];
        p.extend_from_slice(led);
        p.push(persist as u8);
        self.request(FEAT_COLOR_LED_EFFECTS, 3, &p)?;
        Ok(())
    }

    // ---- MouseButtonSpy (0x8110): fn0 count, fn1 start, fn2 stop, fn3/fn4 map ----

    pub fn spy_count(&self) -> Result<u8> {
        Ok(*self.request(FEAT_MOUSE_BUTTON_SPY, 0, &[])?.first().unwrap_or(&0))
    }

    pub fn spy_start(&self) -> Result<()> {
        self.request(FEAT_MOUSE_BUTTON_SPY, 1, &[])?;
        Ok(())
    }

    pub fn spy_stop(&self) -> Result<()> {
        self.request(FEAT_MOUSE_BUTTON_SPY, 2, &[])?;
        Ok(())
    }

    /// Per slot: the HID button it sends natively in host mode (0 = muted).
    pub fn spy_get_map(&self) -> Result<[u8; 16]> {
        let p = self.request(FEAT_MOUSE_BUTTON_SPY, 3, &[0u8; 16])?;
        let mut m = [0u8; 16];
        let n = p.len().min(16);
        m[..n].copy_from_slice(&p[..n]);
        Ok(m)
    }

    pub fn spy_set_map(&self, map: &[u8; 16]) -> Result<()> {
        self.request(FEAT_MOUSE_BUTTON_SPY, 4, map)?;
        Ok(())
    }
}

/// Spy event decoder, split out for tests.
pub fn decode_spy(spy_idx: u8, msg: &[u8]) -> Option<u16> {
    (msg.len() >= 6
        && msg[0] == REPORT_LONG
        && msg[1] == DEV_BROADCAST
        && msg[2] == spy_idx
        && msg[3] == 0x00)
        .then(|| u16::from_be_bytes([msg[4], msg[5]]))
}

/// IFirmwareVersion entity: `[type, prefix[3], number BCD, revision BCD, build BCD u16, ..]`.
/// G HUB shows "U1 " + 0x27 / 0x03 / 0x0010 as "127.3.10".
pub fn format_fw(v: &[u8]) -> String {
    if v.len() < 8 {
        return String::new();
    }
    let prefix = String::from_utf8_lossy(&v[1..4]);
    let digits: String = prefix.chars().filter(|c| c.is_ascii_digit()).collect();
    let bcd = |b: u8| (b >> 4) as u32 * 10 + (b & 0x0F) as u32;
    let build = bcd(v[6]) * 100 + bcd(v[7]);
    if digits.is_empty() {
        format!("{} {}.{}.{}", prefix.trim(), bcd(v[4]), bcd(v[5]), build)
    } else {
        format!("{digits}{:02}.{}.{}", bcd(v[4]), bcd(v[5]), build)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spy_bitmap_is_big_endian() {
        let msg = [0x11, 0xFF, 0x0D, 0x00, 0x01, 0x08, 0, 0];
        assert_eq!(decode_spy(0x0D, &msg), Some(0x0108));
        // a response to our own request (swid != 0) is not an event
        assert_eq!(decode_spy(0x0D, &[0x11, 0xFF, 0x0D, 0x13, 0, 1]), None);
    }

    #[test]
    fn firmware_matches_ghub_display() {
        let main = [0x00, 0x55, 0x31, 0x20, 0x27, 0x03, 0x00, 0x10, 0x01, 0xC0, 0x8B];
        assert_eq!(format_fw(&main), "127.3.10");
        let boot = [0x01, 0x42, 0x4F, 0x54, 0x81, 0x00, 0x00, 0x02];
        assert_eq!(format_fw(&boot), "BOT 81.0.2");
    }
}
