#![allow(clippy::pedantic)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

/// Global mutex to serialize tests that mutate `PLAY_CRT_DATA_DIR`.
/// `cargo test` runs unit tests in parallel; without serialization the
/// env-var would race between tests.
#[allow(dead_code)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Return the Play-CRT data directory.
///
/// Search order (first match wins):
/// 1. `$PLAY_CRT_DATA_DIR` env var (tests / explicit override).
/// 2. Portable mode — if an empty file `portable` or `.portable` sits next
///    to the executable, or `$PLAY_CRT_PORTABLE` is set, use `exe_dir/data`.
/// 3. `ProjectDirs::data_local_dir()` — `~/Library/Application Support/...` on
///    macOS, `~/.local/share/...` on Linux, `%LOCALAPPDATA%` on Windows.
/// 4. `exe_dir/data` fallback.
/// 5. `./data` as last resort.
#[must_use]
pub fn data_dir() -> PathBuf {
    if let Ok(p) = env::var("PLAY_CRT_DATA_DIR") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }

    if env::var("PLAY_CRT_PORTABLE").is_ok() {
        if let Ok(exe) = env::current_exe() {
            if let Some(parent) = exe.parent() {
                return parent.join("data");
            }
        }
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("portable").exists() || parent.join(".portable").exists() {
                return parent.join("data");
            }
        }
    }

    // For `cargo test` the binary lives under `target/debug/deps/`. Using
    // `ProjectDirs` there would pollute the real user data dir during tests,
    // so callers set `PLAY_CRT_DATA_DIR` to a temp dir. No automatic test
    // fallback here — the caller controls isolation via env var. We still
    // keep a heuristic: if `CARGO_MANIFEST_DIR` is set and `PLAY_CRT_DATA_DIR`
    // is not, some test harnesses expect exe-relative `data`; we expose a
    // secondary opt-in via `PLAY_CRT_DATA_DIR` so there is no surprise.
    if let Some(proj) = ProjectDirs::from("com", "batk0", "play-crt") {
        return proj.data_local_dir().to_path_buf();
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("data");
        }
    }

    PathBuf::from("data")
}

#[must_use]
pub fn stories_dir() -> PathBuf {
    data_dir().join("stories")
}

#[must_use]
pub fn basic_dir() -> PathBuf {
    data_dir().join("basic")
}

#[allow(dead_code)]
#[must_use]
pub fn saves_dir(game_id: &str) -> PathBuf {
    data_dir().join("saves").join(sanitize_game_id(game_id))
}

#[must_use]
pub fn downloads_dir() -> PathBuf {
    data_dir().join("downloads")
}

/// Ensure the standard layout exists. Safe to call repeatedly.
pub fn ensure_layout() -> Result<(), String> {
    for dir in [
        data_dir(),
        stories_dir(),
        basic_dir(),
        downloads_dir(),
        data_dir().join("saves"),
    ] {
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    Ok(())
}

/// Sanitise a game id: lowercase alphanum, `-`, `_`; other chars become `_`.
#[must_use]
pub fn sanitize_game_id(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed
    }
}

/// Derive a stable game id from story bytes header.
///
/// Header layout (Z-machine spec):
/// - byte 0: version
/// - bytes 2..4: release (big-endian u16)
/// - bytes 0x12..0x18: 6-byte serial (ASCII)
///
/// If serial is printable non-empty, id is `serial-release` sanitized;
/// otherwise `None` so caller can fall back to the file stem.
#[must_use]
pub fn game_id_from_bytes(data: &[u8]) -> Option<String> {
    if data.len() < 0x18 {
        return None;
    }
    let release = u16::from_be_bytes([data[2], data[3]]);
    let serial_bytes = &data[0x12..0x18];
    let serial_raw: String = serial_bytes
        .iter()
        .map(|&b| b as char)
        .collect();
    let serial_trimmed = serial_raw.trim_matches(|c: char| c == '\0' || c == ' ' || !c.is_ascii_graphic()).trim();
    if serial_trimmed.is_empty() {
        return None;
    }
    // Only accept serials that look like 6 alphanum chars (classic Infocom)
    let printable = serial_trimmed.chars().all(|c| c.is_ascii_alphanumeric());
    if !printable {
        return None;
    }
    let candidate = format!("{serial_trimmed}-{release}");
    Some(sanitize_game_id(&candidate))
}

/// Derive game id from a path: try header serial+release, then stem sanitized.
#[must_use]
pub fn game_id_for_path(path: &Path) -> String {
    if let Ok(data) = fs::read(path) {
        // For .zip, try to extract first entry header — but for id purposes we
        // can just fallback to stem if zip (avoids zip parsing here).
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext != "zip" {
            if let Some(id) = game_id_from_bytes(&data) {
                return id;
            }
        }
    }
    // Fallback: file stem
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        return sanitize_game_id(stem);
    }
    sanitize_game_id(&path.to_string_lossy())
}

/// Like `game_id_for_path` but given bytes + stem hint.
#[allow(dead_code)]
#[must_use]
pub fn game_id_for_bytes_with_stem(data: &[u8], stem: &str) -> String {
    if let Some(id) = game_id_from_bytes(data) {
        return id;
    }
    sanitize_game_id(stem)
}

/// Return path to the bundled manifest (assets/manifests/stories.json).
/// Searches multiple locations for dev vs installed layout.
#[must_use]
pub fn bundled_manifest_path() -> Option<PathBuf> {
    let candidates = manifest_search_paths();
    candidates.into_iter().find(|p| p.exists())
}

fn manifest_search_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    v.push(PathBuf::from("assets/manifests/stories.json"));
    if let Ok(cwd) = env::current_dir() {
        for anc in cwd.ancestors() {
            v.push(anc.join("assets/manifests/stories.json"));
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            v.push(parent.join("assets/manifests/stories.json"));
            v.push(parent.join("../assets/manifests/stories.json"));
            v.push(parent.join("../../assets/manifests/stories.json"));
        }
    }
    v.push(PathBuf::from("/usr/local/share/play-crt/stories.json"));
    v
}

/// Return path to the bundled BASIC manifest (assets/manifests/basic.json).
#[must_use]
pub fn bundled_basic_manifest_path() -> Option<PathBuf> {
    let candidates = basic_manifest_search_paths();
    candidates.into_iter().find(|p| p.exists())
}

fn basic_manifest_search_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    v.push(PathBuf::from("assets/manifests/basic.json"));
    if let Ok(cwd) = env::current_dir() {
        for anc in cwd.ancestors() {
            v.push(anc.join("assets/manifests/basic.json"));
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            v.push(parent.join("assets/manifests/basic.json"));
            v.push(parent.join("../assets/manifests/basic.json"));
            v.push(parent.join("../../assets/manifests/basic.json"));
        }
    }
    v.push(PathBuf::from("/usr/local/share/play-crt/basic.json"));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sanitize_basic() {
        assert_eq!(sanitize_game_id("Zork I"), "zork_i");
        assert_eq!(sanitize_game_id("Hello-World 123"), "hello-world_123");
        assert_eq!(sanitize_game_id("///"), "unknown");
    }

    #[test]
    fn game_id_from_bytes_valid() {
        let mut data = vec![0u8; 0x18];
        data[2] = 0x00;
        data[3] = 0x1B; // release 27
        data[0x12..0x18].copy_from_slice(b"880522");
        let id = game_id_from_bytes(&data).expect("should parse serial");
        assert_eq!(id, "880522-27");
    }

    #[test]
    fn game_id_from_bytes_empty_fallback() {
        let data = vec![0u8; 0x18];
        assert!(game_id_from_bytes(&data).is_none());
        // Stem fallback
        assert_eq!(game_id_for_bytes_with_stem(&data, "My Game"), "my_game");
    }

    #[test]
    fn data_dir_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_test_data_dir_override");
        let _ = fs::create_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let got = data_dir();
        assert_eq!(got, tmp);
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_layout_creates_dirs() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_test_ensure_layout");
        let _ = fs::remove_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        ensure_layout().expect("ensure_layout");
        assert!(tmp.join("stories").exists());
        assert!(tmp.join("downloads").exists());
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn saves_dir_is_sanitized() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        let tmp = std::env::temp_dir().join("play_crt_test_saves");
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let p = saves_dir("ZORK I");
        assert!(p.ends_with("saves/zork_i"), "got {}", p.display());
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
    }
}
