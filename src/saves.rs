#![allow(clippy::pedantic)]
#![allow(clippy::nursery)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths;

/// Number of save slots per game.
pub const NUM_SLOTS: u8 = 3;

/// Metadata for a single slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveMeta {
    pub slot: u8,
    pub status: String,
    pub timestamp: String,
    pub size: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sidecar {
    status: String,
    timestamp: String,
    size: u64,
}

/// Return the path for `slot` (1..=NUM_SLOTS) for the given `game_id`.
#[must_use]
pub fn slot_path(game_id: &str, slot: u8) -> PathBuf {
    assert!(
        (1..=NUM_SLOTS).contains(&slot),
        "slot out of range 1..={NUM_SLOTS}: {slot}"
    );
    paths::saves_dir(game_id).join(format!("slot{slot}.qz"))
}

/// Sidecar json path for a slot.
fn sidecar_path(game_id: &str, slot: u8) -> PathBuf {
    assert!((1..=NUM_SLOTS).contains(&slot));
    paths::saves_dir(game_id).join(format!("slot{slot}.json"))
}

/// Return whether a slot file exists.
#[allow(dead_code)]
#[must_use]
pub fn slot_exists(game_id: &str, slot: u8) -> bool {
    slot_path(game_id, slot).exists()
}

/// Read raw Quetzal bytes for a slot, if present.
#[must_use]
pub fn read_slot(game_id: &str, slot: u8) -> Option<Vec<u8>> {
    let p = slot_path(game_id, slot);
    match fs::read(&p) {
        Ok(b) if !b.is_empty() => Some(b),
        _ => None,
    }
}

/// Write Quetzal bytes to the selected slot plus a sidecar JSON with status.
pub fn write_slot(game_id: &str, slot: u8, quetzal_bytes: &[u8], status: &str) -> Result<(), String> {
    assert!((1..=NUM_SLOTS).contains(&slot));
    let dir = paths::saves_dir(game_id);
    fs::create_dir_all(&dir).map_err(|e| format!("create saves dir {}: {e}", dir.display()))?;
    let path = slot_path(game_id, slot);
    fs::write(&path, quetzal_bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    let ts = now_timestamp();
    let side = Sidecar {
        status: status.to_string(),
        timestamp: ts.clone(),
        size: quetzal_bytes.len() as u64,
    };
    let json_path = sidecar_path(game_id, slot);
    let json = serde_json::to_string_pretty(&side).map_err(|e| format!("json encode: {e}"))?;
    // Best-effort sidecar write; failure is not fatal for the save itself.
    // On failure remove any stale sidecar so metadata does not desync.
    if fs::write(&json_path, &json).is_err() {
        let _ = fs::remove_file(&json_path);
    }
    Ok(())
}

/// List slots 1..=NUM_SLOTS. Each entry is `Some(meta)` if occupied, `None` if empty.
/// Always returns length `NUM_SLOTS`.
#[must_use]
pub fn list_slots(game_id: &str) -> Vec<Option<SaveMeta>> {
    let mut out = Vec::with_capacity(NUM_SLOTS as usize);
    for slot in 1..=NUM_SLOTS {
        let path = slot_path(game_id, slot);
        if !path.exists() {
            out.push(None);
            continue;
        }
        let meta = build_meta(game_id, slot, &path);
        out.push(Some(meta));
    }
    out
}

fn build_meta(game_id: &str, slot: u8, path: &Path) -> SaveMeta {
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    // Try sidecar first
    let side_path = sidecar_path(game_id, slot);
    if let Ok(text) = fs::read_to_string(&side_path) {
        if let Ok(side) = serde_json::from_str::<Sidecar>(&text) {
            return SaveMeta {
                slot,
                status: side.status,
                timestamp: side.timestamp,
                size: side.size,
                path: path.to_path_buf(),
            };
        }
    }
    // Fallback: use file mtime and generic status
    let ts = file_mtime_string(path).unwrap_or_else(now_timestamp);
    SaveMeta {
        slot,
        status: "Occupied".to_string(),
        timestamp: ts,
        size,
        path: path.to_path_buf(),
    }
}

fn file_mtime_string(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    Some(format_system_time(mtime))
}

fn now_timestamp() -> String {
    format_system_time(SystemTime::now())
}

fn format_system_time(t: SystemTime) -> String {
    // Produce "YYYY-MM-DD HH:MM" in local-ish time without adding chrono.
    // Use duration since epoch to derive a simple UTC date.
    // This is approximate but deterministic for display/tests.
    let dur = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    // Very small calendar conversion (UTC) for display; acceptable for slot UI.
    // Use proleptic Gregorian via simple algorithm.
    let (y, m, d, hh, mm) = secs_to_ymd_hm(secs);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

// Convert seconds since 1970-01-01 00:00:00 UTC to Y/M/D H:M.
fn secs_to_ymd_hm(secs: u64) -> (i32, u32, u32, u32, u32) {
    let secs_per_day: u64 = 86_400;
    let days = (secs / secs_per_day) as i64;
    let rem = (secs % secs_per_day) as u32;
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    // civil_from_days from Howard Hinnant
    let (y, m, d) = civil_from_days(days + 719_468); // days since 0000-03-01
    (y, m, d, hh, mm)
}

// Algorithm from https://howardhinnant.github.io/date_algorithms.html#civil_from_days
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0,399]
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0,365]
    let mp = (5 * doy + 2) / 153; // [0,11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1,31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1,12]
    y += i64::from(m <= 2);
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_game_id() -> String {
        format!("test_save_{}", std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())
    }

    #[test]
    fn slot_path_is_sanitized() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        let tmp = std::env::temp_dir().join("play_crt_saves_slot_path");
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let p = slot_path("ZORK I", 1);
        assert!(p.ends_with("saves/zork_i/slot1.qz"), "got {}", p.display());
        assert_eq!(slot_path("zork1", 2).file_name().unwrap(), "slot2.qz");
        if let Some(v) = prev { std::env::set_var("PLAY_CRT_DATA_DIR", v); } else { std::env::remove_var("PLAY_CRT_DATA_DIR"); }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn write_and_read_roundtrip() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("play_crt_saves_rt_{}", std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let _ = fs::remove_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let gid = tmp_game_id();
        let bytes = b"FAKEQUETZAL";
        write_slot(&gid, 1, bytes, "West of House - 10/0").unwrap();
        assert!(slot_exists(&gid, 1));
        assert!(!slot_exists(&gid, 2));
        let got = read_slot(&gid, 1).expect("should exist");
        assert_eq!(got, bytes);
        assert!(read_slot(&gid, 2).is_none());
        let list = list_slots(&gid);
        assert_eq!(list.len(), 3);
        assert!(list[0].is_some());
        assert!(list[1].is_none());
        let meta = list[0].as_ref().unwrap();
        assert_eq!(meta.slot, 1);
        assert_eq!(meta.status, "West of House - 10/0");
        assert!(meta.size == bytes.len() as u64);
        assert!(!meta.timestamp.is_empty());
        // Overwrite slot 1 with different bytes
        let bytes2 = b"OTHER";
        write_slot(&gid, 1, bytes2, "North of House - 5/1").unwrap();
        let got2 = read_slot(&gid, 1).unwrap();
        assert_eq!(got2, bytes2);
        let list2 = list_slots(&gid);
        assert_eq!(list2[0].as_ref().unwrap().status, "North of House - 5/1");
        if let Some(v) = prev { std::env::set_var("PLAY_CRT_DATA_DIR", v); } else { std::env::remove_var("PLAY_CRT_DATA_DIR"); }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn list_slots_empty() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("play_crt_saves_empty_{}", std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let _ = fs::remove_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let gid = tmp_game_id();
        let list = list_slots(&gid);
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|e| e.is_none()));
        if let Some(v) = prev { std::env::set_var("PLAY_CRT_DATA_DIR", v); } else { std::env::remove_var("PLAY_CRT_DATA_DIR"); }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sidecar_fallback_when_missing() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("play_crt_saves_fallback_{}", std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let _ = fs::remove_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let gid = tmp_game_id();
        // Write slot bytes directly without sidecar
        let dir = crate::paths::saves_dir(&gid);
        fs::create_dir_all(&dir).unwrap();
        let p = slot_path(&gid, 2);
        fs::write(&p, b"RAW").unwrap();
        let list = list_slots(&gid);
        let meta = list[1].as_ref().expect("slot2 should be occupied");
        assert_eq!(meta.status, "Occupied");
        assert_eq!(meta.size, 3);
        if let Some(v) = prev { std::env::set_var("PLAY_CRT_DATA_DIR", v); } else { std::env::remove_var("PLAY_CRT_DATA_DIR"); }
        let _ = fs::remove_dir_all(&tmp);
    }
}
