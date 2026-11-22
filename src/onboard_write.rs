//! Writing the mouse's on-board memory: backup, write, read back, verify, roll back.
//! Shared by the IO thread and the `probe onboard-*` commands.

use crate::device::{G502, MODE_ONBOARD};
use crate::profile::{Config, SECTOR_SIZE, sector_crc_ok};
use std::path::{Path, PathBuf};

pub type Sector = [u8; SECTOR_SIZE];

/// Sectors 0..=5 hold the directory and the five profiles.
pub const SECTORS: std::ops::RangeInclusive<u16> = 0..=5;

/// Folder for backups: `<config dir>\onboard_backup` (honours `GHUBX_CONFIG_DIR`).
pub fn backup_root() -> PathBuf {
    Config::path().parent().map(Path::to_path_buf).unwrap_or_default().join("onboard_backup")
}

fn stamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}_{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// A sector read twice until both reads agree.
pub fn read_stable(dev: &G502, sector: u16) -> Result<Sector, String> {
    for _ in 0..3 {
        let a = dev.read_sector(sector).map_err(|e| format!("read sector {sector}: {e}"))?;
        let b = dev.read_sector(sector).map_err(|e| format!("read sector {sector}: {e}"))?;
        if a == b {
            return Ok(a);
        }
    }
    Err(format!("sector {sector} changes between reads"))
}

/// Read sectors 0..=5 and save them with a manifest into `<dir>\<timestamp>` (or `dir` itself when `exact`).
pub fn backup(dev: &G502, dir: &Path, exact: bool) -> Result<(PathBuf, Vec<Sector>), String> {
    let target = if exact { dir.to_path_buf() } else { dir.join(stamp()) };
    std::fs::create_dir_all(&target).map_err(|e| format!("create {}: {e}", target.display()))?;
    let mut all = Vec::new();
    let mut manifest = format!("{{\n  \"created\": \"{}\",\n  \"firmware\": \"{}\",\n  \"sectors\": [\n", stamp(), dev.info.fw);
    for s in SECTORS {
        let data = read_stable(dev, s)?;
        std::fs::write(target.join(format!("sector_{s:04X}.bin")), data).map_err(|e| format!("write backup: {e}"))?;
        manifest += &format!("    {{\"sector\": {s}, \"crc_ok\": {}}}{}\n", sector_crc_ok(&data), if s == *SECTORS.end() { "" } else { "," });
        all.push(data);
    }
    manifest += "  ]\n}\n";
    std::fs::write(target.join("manifest.json"), manifest).map_err(|e| format!("write manifest: {e}"))?;
    Ok((target, all))
}

/// Back up, write every sector that differs, verify each by reading it back (one retry, then roll
/// back to the backup and stop), optionally make `active` the active on-board slot.
pub fn write_sectors(dev: &G502, sectors: &[(u16, Sector)], active: Option<u8>, log: &mut dyn FnMut(String)) -> Result<String, String> {
    if crate::win::ghub_running() {
        return Err("G HUB is running".into());
    }
    for (s, d) in sectors {
        if !SECTORS.contains(s) {
            return Err(format!("sector {s} is outside 0..=5"));
        }
        if !sector_crc_ok(d) {
            return Err(format!("sector {s} has a bad CRC"));
        }
    }
    let (dir, current) = backup(dev, &backup_root(), false)?;
    log(format!("backup saved to {}", dir.display()));
    let mut written = 0;
    for (s, data) in sectors {
        if current[*s as usize] == *data {
            log(format!("sector {s}: already identical"));
            continue;
        }
        let mut ok = false;
        for attempt in 1..=2 {
            dev.write_sector(*s, data).map_err(|e| format!("write sector {s}: {e}"))?;
            let back = read_stable(dev, *s)?;
            if back == *data {
                ok = true;
                break;
            }
            log(format!("sector {s}: verify failed (attempt {attempt})"));
        }
        if !ok {
            let _ = dev.write_sector(*s, &current[*s as usize]);
            return Err(format!("sector {s} did not verify after two writes; the backup copy was written back ({})", dir.display()));
        }
        log(format!("sector {s}: written and verified"));
        written += 1;
    }
    if let Some(slot) = active {
        dev.set_mode(MODE_ONBOARD).map_err(|e| format!("on-board mode: {e}"))?;
        dev.set_onboard_profile(slot).map_err(|e| format!("select slot: {e}"))?;
        let now = dev.get_onboard_profile().map_err(|e| format!("read active slot: {e}"))?;
        if now != slot {
            return Err(format!("active slot reads {now}, expected {slot}"));
        }
        log(format!("active slot is {slot}"));
    }
    Ok(format!("{written} sector(s) written and checked; backup in {}", dir.display()))
}

/// Copy the factory ROM profile (sector 0x0100 + slot) over each given user slot (1..=5).
pub fn restore_slots(dev: &G502, slots: &[u16], log: &mut dyn FnMut(String)) -> Result<String, String> {
    let mut sectors = Vec::new();
    for &n in slots {
        if !(1..=5).contains(&n) {
            return Err(format!("slot {n} is outside 1..=5"));
        }
        sectors.push((n, crate::profile::restore_from_rom(&read_stable(dev, 0x0100 + n)?)));
    }
    write_sectors(dev, &sectors, None, log)
}
