//! HID++ 2.0 protocol core for Logitech devices.
//!
//! Wire format (report ID included):
//!   short: [0x10, dev_idx, feat_idx, func_swid, p0, p1, p2]      (7 bytes)
//!   long:  [0x11, dev_idx, feat_idx, func_swid, p0..p15]        (20 bytes)
//! func_swid = (function_id << 4) | software_id
//! Error frames have feat_idx == 0x8F (short) / 0xFF (long),
//! with [orig_feat_idx, orig_func_swid, err_code] following.

use hidapi::{HidApi, HidDevice};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use thiserror::Error;

pub const REPORT_SHORT: u8 = 0x10;
pub const REPORT_LONG: u8 = 0x11;
pub const ERROR_MARKER_SHORT: u8 = 0x8F;
pub const ERROR_MARKER_LONG: u8 = 0xFF;

pub const DEV_BROADCAST: u8 = 0xFF;

pub const FEAT_ROOT: u16 = 0x0000;
pub const FEAT_FEATURE_SET: u16 = 0x0001;
pub const FEAT_DEVICE_FW_VERSION: u16 = 0x0003;
pub const FEAT_DEVICE_NAME: u16 = 0x0005;
pub const FEAT_DEVICE_GROUPS: u16 = 0x0007;
pub const FEAT_DEVICE_TYPE_AND_NAME: u16 = 0x0023;
pub const FEAT_RESET: u16 = 0x0020;
pub const FEAT_DFU: u16 = 0x00D0;
pub const FEAT_BATTERY: u16 = 0x1000;
pub const FEAT_REPROG_CONTROLS: u16 = 0x1B00;
pub const FEAT_REPROG_CONTROLS_V2_2: u16 = 0x1B01;
pub const FEAT_REPROG_CONTROLS_V3: u16 = 0x1B03;
pub const FEAT_REPROG_CONTROLS_V4: u16 = 0x1B04;
pub const FEAT_K375S_FN: u16 = 0x40A3;
pub const FEAT_ADJUSTABLE_DPI: u16 = 0x2201;
pub const FEAT_SMART_SHIFT: u16 = 0x2110;
pub const FEAT_HIRES_SCROLL: u16 = 0x2120;
pub const FEAT_HIRES_SCROLL_V2: u16 = 0x2121;
pub const FEAT_LOWRES_WHEEL: u16 = 0x2130;
pub const FEAT_ADJUSTABLE_REPORT_RATE: u16 = 0x8060;
pub const FEAT_COLOR_LED_EFFECTS: u16 = 0x8070;
pub const FEAT_RGB_EFFECTS: u16 = 0x8071;
pub const FEAT_PER_KEY_LIGHTING: u16 = 0x8081;
pub const FEAT_ONBOARD_PROFILES_V0: u16 = 0x8060;
pub const FEAT_ONBOARD_PROFILES: u16 = 0x8100;
pub const FEAT_MOUSE_BUTTON_SPY: u16 = 0x8110;
pub const FEAT_FORCE_FEEDBACK: u16 = 0x8123;
pub const FEAT_DEVICE_CONFIG: u16 = 0x1802;

#[derive(Debug, Error)]
pub enum HidppError {
    #[error("hid error: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("timeout waiting for response")]
    Timeout,
    #[error("device returned error {code} (feature 0x{feat:02X} fn {func})")]
    Device { code: u8, feat: u8, func: u8 },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("feature 0x{0:04X} not supported")]
    FeatureNotSupported(u16),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, HidppError>;

/// A single HID++ transaction channel.
/// G502 wired: writes go out on the usage-0x0001 vendor collection,
/// responses/events arrive on the usage-0x0002 collection.
pub struct HidppDevice<'a> {
    w: &'a HidDevice,
    r: &'a HidDevice,
    sw_id: AtomicU8,
    pub dev_index: u8,
    /// Non-matching frames seen while waiting for a response
    /// (button-spy events, G HUB traffic). Drained by the owner thread.
    stray: std::sync::Arc<std::sync::Mutex<VecDeque<Vec<u8>>>>,
}

impl<'a> HidppDevice<'a> {
    pub fn new(w: &'a HidDevice, r: &'a HidDevice, dev_index: u8) -> Self {
        Self::with_stray(w, r, dev_index, Default::default())
    }

    pub fn with_stray(
        w: &'a HidDevice,
        r: &'a HidDevice,
        dev_index: u8,
        stray: std::sync::Arc<std::sync::Mutex<VecDeque<Vec<u8>>>>,
    ) -> Self {
        Self {
            w,
            r,
            sw_id: AtomicU8::new(1),
            dev_index,
            stray,
        }
    }

    /// Frames that arrived during `request` but weren't our response.
    pub fn drain_stray(&self) -> Vec<Vec<u8>> {
        self.stray.lock().unwrap().drain(..).collect()
    }

    fn push_stray(&self, msg: &[u8]) {
        let mut q = self.stray.lock().unwrap();
        if q.len() < 256 {
            q.push_back(msg.to_vec());
        }
    }

    fn next_sw_id(&self) -> u8 {
        // software id is 4 bits, keep it nonzero
        self.sw_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(if v >= 0x0F { 1 } else { v + 1 })
            })
            .unwrap_or(1)
    }

    /// Send a request and wait for the matching response.
    /// `params` ≤3 bytes -> short report (0x10) written on the usage-1 handle.
    /// `params` 4..=16 -> long report (0x11) written on the usage-2 handle.
    /// All responses arrive on the usage-2 handle.
    pub fn request(&self, feat_idx: u8, func: u8, params: &[u8]) -> Result<Vec<u8>> {
        let sw_id = self.next_sw_id();
        let func_swid = (func << 4) | sw_id;

        let mut buf = Vec::with_capacity(20);
        let long = params.len() > 3;
        buf.push(if long { REPORT_LONG } else { REPORT_SHORT });
        buf.push(self.dev_index);
        buf.push(feat_idx);
        buf.push(func_swid);
        buf.extend_from_slice(params);

        if long {
            self.r.write(&buf)?;
        } else {
            self.w.write(&buf)?;
        }

        let deadline = std::time::Instant::now() + Duration::from_millis(1500);
        let mut rbuf = [0u8; 64];
        loop {
            let n = self.r.read_timeout(&mut rbuf, 200).map_err(HidppError::Hid)?;
            if n == 0 {
                if std::time::Instant::now() > deadline {
                    return Err(HidppError::Timeout);
                }
                continue;
            }
            let msg = &rbuf[..n];
            if msg.len() < 5 || (msg[0] != REPORT_SHORT && msg[0] != REPORT_LONG) {
                self.push_stray(msg);
                continue;
            }
            if msg[1] != self.dev_index {
                self.push_stray(msg);
                continue;
            }
            // error frame?
            if msg[2] == ERROR_MARKER_SHORT || msg[2] == ERROR_MARKER_LONG {
                let orig_feat = msg[3];
                let orig_func_swid = msg[4];
                if orig_feat == feat_idx && orig_func_swid == func_swid {
                    let code = *msg.get(5).unwrap_or(&0);
                    return Err(HidppError::Device {
                        code,
                        feat: feat_idx,
                        func,
                    });
                }
                self.push_stray(msg);
                continue;
            }
            if msg[2] != feat_idx || msg[3] != func_swid {
                self.push_stray(msg);
                continue;
            }
            return Ok(msg[4..].to_vec());
        }
    }

    /// ROOT fn 0x01 (get protocol version).
    pub fn protocol_version(&self) -> Result<(u8, u8)> {
        let p = self.request(0x00, 0x01, &[])?;
        Ok((*p.first().unwrap_or(&0), *p.get(1).unwrap_or(&0)))
    }

    /// Read one frame off the event handle without matching a request.
    /// Returns the raw message, or None on timeout.
    pub fn poll_event(&self, ms: i32) -> Option<Vec<u8>> {
        let mut b = [0u8; 64];
        match self.r.read_timeout(&mut b, ms) {
            Ok(n) if n > 0 => Some(b[..n].to_vec()),
            _ => None,
        }
    }

    /// ROOT getFeature(feature_id) -> index (0 = unsupported).
    /// Root layout: fn 0x00 takes [id_msb, id_lsb] -> [index, type, version].
    pub fn feature_index(&self, feature_id: u16) -> Result<u8> {
        let p = self.request(0x00, 0x00, &[
            (feature_id >> 8) as u8,
            (feature_id & 0xFF) as u8,
        ])?;
        Ok(*p.first().unwrap_or(&0))
    }
}

/// Info about one enumerated feature.
#[derive(Debug, Clone)]
pub struct FeatureInfo {
    pub index: u8,
    pub id: u16,
    pub ty: u8,
    pub version: u8,
}

/// Enumerate the device's feature table via FEATURE_SET (0x0001).
/// G502 Hero quirk: ROOT.getFeature is non-functional (returns constant
/// [04,02] for every input), so we can't discover FeatureSet by ID.
/// FeatureSet canonically sits at index 1 on HID++ 2.0 devices; we
/// validate whatever index we use by requiring the table to contain
/// FeatureSet's own ID.
pub fn enumerate_features(dev: &HidppDevice) -> Result<Vec<FeatureInfo>> {
    let mut tried: Vec<u8> = Vec::new();
    // candidate indexes: root lookup first, then canonical 1, then scan
    let mut candidates: Vec<u8> = Vec::new();
    if let Ok(idx) = dev.feature_index(FEAT_FEATURE_SET) {
        candidates.push(idx);
    }
    candidates.push(1);
    candidates.extend(2..=16u8);

    for fs_idx in candidates {
        if fs_idx == 0 || tried.contains(&fs_idx) {
            continue;
        }
        tried.push(fs_idx);
        if let Ok(feats) = enumerate_at(dev, fs_idx) {
            // validation: table must contain FeatureSet's own id
            if feats.iter().any(|f| f.id == FEAT_FEATURE_SET) {
                return Ok(feats);
            }
        }
    }
    Err(HidppError::FeatureNotSupported(FEAT_FEATURE_SET))
}

fn enumerate_at(dev: &HidppDevice, fs_idx: u8) -> Result<Vec<FeatureInfo>> {
    // fn 0x00: getCount
    let count = *dev.request(fs_idx, 0x00, &[])?.first().unwrap_or(&0);
    if count == 0 || count > 64 {
        return Err(HidppError::Other(format!("implausible feature count {count}")));
    }
    let mut out = Vec::new();
    for i in 0..count {
        // fn 0x01: getFeatureID(index) -> [id_hi, id_lo, type, version]
        let p = dev.request(fs_idx, 0x01, &[i])?;
        if p.len() >= 3 {
            // G502 Hero quirk: feature index == table position (0-based)
            out.push(FeatureInfo {
                index: i,
                id: ((p[0] as u16) << 8) | p[1] as u16,
                ty: p[2],
                version: p[3],
            });
        }
    }
    Ok(out)
}

/// Resolve a feature id to its runtime index by scanning the table.
pub fn resolve_feature(_dev: &HidppDevice, table: &[FeatureInfo], id: u16) -> Result<u8> {
    table
        .iter()
        .find(|f| f.id == id)
        .map(|f| f.index)
        .ok_or(HidppError::FeatureNotSupported(id))
}

/// Find the G502's HID++ vendor interface. Returns open devices to try.
pub fn open_candidates(api: &HidApi, vid: u16, pid: u16) -> Vec<(hidapi::DeviceInfo, String)> {
    api.device_list()
        .filter(|d| d.vendor_id() == vid && d.product_id() == pid)
        .filter(|d| d.usage_page() >= 0xFF00)
        .map(|d| {
            (
                d.clone(),
                format!(
                    "if{} usage_page=0x{:04X} usage=0x{:04X}",
                    d.interface_number(),
                    d.usage_page(),
                    d.usage()
                ),
            )
        })
        .collect()
}

/// Error code table (HID++ 2.0)
pub fn err_name(code: u8) -> &'static str {
    match code {
        0x01 => "SUCCESS?",
        0x02 => "INVALID_SUBID",
        0x03 => "INVALID_ADDRESS",
        0x04 => "INVALID_VALUE",
        0x05 => "CONNECT_FAIL?",
        0x06 => "TOO_MANY_DEVICES",
        0x07 => "ALREADY_EXISTS",
        0x08 => "BUSY",
        0x09 => "UNKNOWN_DEVICE",
        0x0A => "RESOURCE_ERROR",
        0x0B => "REQUEST_UNAVAILABLE",
        0x0C => "INVALID_PARAM_VALUE",
        0x0D => "WRONG_PIN_CODE",
        _ => "?",
    }
}
