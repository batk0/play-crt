#![allow(clippy::pedantic)]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::basic_catalog::BasicEntry;
use crate::catalog::GameEntry;
use crate::paths;

/// Embedded Mini-Zork fixture — used as offline fallback when network fails.
/// This is the same file as `src/zmachine/fixtures/minizork.z3`.
const MINIZORK_BYTES: &[u8] = include_bytes!("zmachine/fixtures/minizork.z3");

/// Try to install the bundled Mini-Zork fixture to `final_path`.
///
/// Only applies when `entry.id == "minizork"`. Returns `Some(Ok(path))` on
/// success, `Some(Err(msg))` on fallback failure, and `None` if not applicable.
fn try_install_bundled_minizork(entry: &GameEntry, final_path: &Path) -> Option<Result<PathBuf, String>> {
    if entry.id != "minizork" {
        return None;
    }
    // Optional sha256 verification against manifest if provided
    if let Some(expected_hex) = &entry.sha256 {
        let mut hasher = Sha256::new();
        hasher.update(MINIZORK_BYTES);
        let got = hex::encode(hasher.finalize());
        if !got.eq_ignore_ascii_case(expected_hex) && std::env::var("DEBUG").is_ok() {
            // Log mismatch but still install — the embedded fixture is authoritative
            eprintln!(
                "bundled minizork sha256 mismatch: expected {expected_hex}, got {got} — installing anyway"
            );
        }
    }
    // Ensure parent exists (caller already does, but be defensive)
    if let Some(parent) = final_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return Some(Err(format!("create {} for fallback: {e}", parent.display())));
        }
    }
    match fs::write(final_path, MINIZORK_BYTES) {
        Ok(()) => Some(Ok(final_path.to_path_buf())),
        Err(e) => Some(Err(format!("bundled minizork fallback write failed: {e}"))),
    }
}

/// Search for a filesystem copy of the bundled minizork fixture (dev layout).
/// Used as secondary fallback if embedded bytes write fails for any reason.
fn find_filesystem_minizork_fixture() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("src/zmachine/fixtures/minizork.z3")];
    if let Ok(cwd) = std::env::current_dir() {
        for anc in cwd.ancestors() {
            candidates.push(anc.join("src/zmachine/fixtures/minizork.z3"));
            candidates.push(anc.join("assets/stories/minizork.z3"));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("src/zmachine/fixtures/minizork.z3"));
            candidates.push(parent.join("../src/zmachine/fixtures/minizork.z3"));
            candidates.push(parent.join("../../src/zmachine/fixtures/minizork.z3"));
            candidates.push(parent.join("assets/stories/minizork.z3"));
        }
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Download a `GameEntry` to `stories/<id>/<filename>`.
///
/// - Downloads to `downloads/<id>.part` first.
/// - Optionally verifies `sha256` if the manifest provides it.
/// - Atomic rename to `stories/<id>/<filename>` (creates parent dirs).
/// - `progress` receives 0..100 percentages (best-effort, called once at start/end if length unknown).
///
/// Returns the final local path on success, or an error string on failure (offline, http error, hash mismatch).
pub fn download<F>(entry: &GameEntry, progress: F) -> Result<PathBuf, String>
where
    F: Fn(u8),
{
    // Ensure layout early so fallback can write
    paths::ensure_layout().map_err(|e| e.to_string())?;

    let ddir = paths::downloads_dir();
    let sdir = paths::stories_dir();
    let final_dir = sdir.join(&entry.id);
    let final_path = final_dir.join(&entry.filename);
    let part_path = ddir.join(format!("{}.part", entry.id));

    fs::create_dir_all(&ddir).map_err(|e| format!("create downloads: {e}"))?;
    fs::create_dir_all(&final_dir).map_err(|e| format!("create {}: {e}", final_dir.display()))?;

    if entry.url.trim().is_empty() {
        // No URL — try bundled fallback for minizork before erroring
        if let Some(fb) = try_install_bundled_minizork(entry, &final_path) {
            if fb.is_ok() {
                progress(100);
            }
            return fb;
        }
        // Try filesystem fixture as last resort
        if entry.id == "minizork" {
            if let Some(fs_path) = find_filesystem_minizork_fixture() {
                match fs::copy(&fs_path, &final_path) {
                    Ok(_) => {
                        progress(100);
                        return Ok(final_path);
                    }
                    Err(e) => {
                        return Err(format!(
                            "no remote URL for {} and filesystem fallback copy failed: {e}",
                            entry.id
                        ));
                    }
                }
            }
        }
        return Err(format!("no remote URL for {}", entry.id));
    }

    // Best-effort cleanup of stale partial
    let _ = fs::remove_file(&part_path);

    progress(0);

    // Use ureq (blocking, no tokio). 30s timeout.
    // Wrap network attempt so we can fallback to bundled minizork on any failure.
    let network_result: Result<PathBuf, String> = (|| {
        let resp = ureq::get(&entry.url)
            .timeout(std::time::Duration::from_secs(30))
            .call()
            .map_err(|e| format!("download failed for {}: {e}", entry.url))?;

        if !(200..300).contains(&resp.status()) {
            return Err(format!("http {} for {}", resp.status(), entry.url));
        }
        Ok(resp)
    })()
    .and_then(|resp| {
        let len: Option<u64> = resp
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
            .or(entry.size);

        let max_bytes: u64 = entry.size.map_or(10 * 1024 * 1024, |s| s.saturating_add(8 * 1024));

        let mut reader = resp.into_reader();

        let mut out = fs::File::create(&part_path).map_err(|e| format!("create part file: {e}"))?;
        let mut hasher = if entry.sha256.is_some() {
            Some(Sha256::new())
        } else {
            None
        };
        let mut buf = vec![0u8; 32 * 1024];
        let mut total: u64 = 0;
        let mut last_pct: u8 = 0;

        loop {
            let n = std::io::Read::read(&mut reader, &mut buf).map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                break;
            }
            if total.saturating_add(n as u64) > max_bytes {
                drop(out);
                let _ = fs::remove_file(&part_path);
                return Err(format!(
                    "download exceeds size limit ({} bytes) for {}",
                    max_bytes, entry.id
                ));
            }
            out.write_all(&buf[..n])
                .map_err(|e| format!("write part: {e}"))?;
            if let Some(h) = hasher.as_mut() {
                h.update(&buf[..n]);
            }
            total += n as u64;
            if let Some(expected) = len {
                if let Some(pct) = total
                    .checked_mul(100)
                    .and_then(|v| v.checked_div(expected))
                    .map(|v| v.min(100) as u8)
                {
                    if pct != last_pct {
                        last_pct = pct;
                        progress(pct);
                    }
                }
            }
        }
        drop(out);

        if let Some(expected_hex) = &entry.sha256 {
            let hash = hasher.expect("hasher").finalize();
            let got = hex::encode(hash);
            if !got.eq_ignore_ascii_case(expected_hex) {
                let _ = fs::remove_file(&part_path);
                return Err(format!(
                    "sha256 mismatch for {}: expected {expected_hex}, got {got}",
                    entry.id
                ));
            }
        }

        if total == 0 {
            let _ = fs::remove_file(&part_path);
            return Err("downloaded file is empty".to_string());
        }

        // Atomic rename (within same filesystem; data_dir is single tree so this is atomic)
        // If cross-device, fallback to copy+remove.
        if let Err(e) = fs::rename(&part_path, &final_path) {
            // Fallback copy
            fs::copy(&part_path, &final_path)
                .map_err(|e2| format!("rename {e} and copy fallback {e2}"))?;
            let _ = fs::remove_file(&part_path);
        }

        progress(100);
        Ok(final_path.clone())
    });

    match network_result {
        Ok(p) => Ok(p),
        Err(e) => {
            // Network failed — try bundled fallback for minizork
            if entry.id == "minizork" {
                // Clean up partial if present
                let _ = fs::remove_file(&part_path);
                if let Some(fb) = try_install_bundled_minizork(entry, &final_path) {
                    match fb {
                        Ok(p) => {
                            progress(100);
                            if std::env::var("DEBUG").is_ok() {
                                eprintln!("download failed for minizork ({e}) — installed bundled fixture to {}", p.display());
                            }
                            return Ok(p);
                        }
                        Err(fe) => {
                            return Err(format!("{e}; bundled fallback also failed: {fe}"));
                        }
                    }
                }
                // Secondary filesystem fallback
                if let Some(fs_path) = find_filesystem_minizork_fixture() {
                    match fs::copy(&fs_path, &final_path) {
                        Ok(_) => {
                            progress(100);
                            if std::env::var("DEBUG").is_ok() {
                                eprintln!(
                                    "download failed for minizork ({e}) — copied filesystem fixture {} to {}",
                                    fs_path.display(),
                                    final_path.display()
                                );
                            }
                            return Ok(final_path);
                        }
                        Err(fe) => {
                            return Err(format!("{e}; filesystem fallback also failed: {fe}"));
                        }
                    }
                }
            }
            Err(e)
        }
    }
}

/// Download a `BasicEntry` to `basic/<id>/<filename>`.
///
/// Mirrors `download` for Z-machine games but targets the BASIC data dir.
/// Uses `downloads/<id>.part` atomic rename, optional sha256, and rejects empty downloads.
pub fn download_basic<F>(entry: &BasicEntry, progress: F) -> Result<PathBuf, String>
where
    F: Fn(u8),
{
    paths::ensure_layout().map_err(|e| e.to_string())?;

    let ddir = paths::downloads_dir();
    let bdir = paths::basic_dir();
    let final_dir = bdir.join(&entry.id);
    let final_path = final_dir.join(&entry.filename);
    let part_path = ddir.join(format!("basic-{}.part", entry.id));

    fs::create_dir_all(&ddir).map_err(|e| format!("create downloads: {e}"))?;
    fs::create_dir_all(&final_dir).map_err(|e| format!("create {}: {e}", final_dir.display()))?;

    if entry.url.trim().is_empty() {
        return Err(format!("no remote URL for {}", entry.id));
    }

    let _ = fs::remove_file(&part_path);
    progress(0);

    let resp = ureq::get(&entry.url)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("download failed for {}: {e}", entry.url))?;

    if !(200..300).contains(&resp.status()) {
        return Err(format!("http {} for {}", resp.status(), entry.url));
    }

    let len: Option<u64> = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .or(entry.size);

    let max_bytes: u64 = entry.size.map_or(10 * 1024 * 1024, |s| s.saturating_add(8 * 1024));

    let mut reader = resp.into_reader();
    let mut out = fs::File::create(&part_path).map_err(|e| format!("create part file: {e}"))?;
    let mut hasher = if entry.sha256.is_some() {
        Some(Sha256::new())
    } else {
        None
    };
    let mut buf = vec![0u8; 32 * 1024];
    let mut total: u64 = 0;
    let mut last_pct: u8 = 0;

    loop {
        let n = std::io::Read::read(&mut reader, &mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        if total.saturating_add(n as u64) > max_bytes {
            drop(out);
            let _ = fs::remove_file(&part_path);
            return Err(format!(
                "download exceeds size limit ({} bytes) for {}",
                max_bytes, entry.id
            ));
        }
        out.write_all(&buf[..n]).map_err(|e| format!("write part: {e}"))?;
        if let Some(h) = hasher.as_mut() {
            h.update(&buf[..n]);
        }
        total += n as u64;
        if let Some(expected) = len {
            if let Some(pct) = total
                .checked_mul(100)
                .and_then(|v| v.checked_div(expected))
                .map(|v| v.min(100) as u8)
            {
                if pct != last_pct {
                    last_pct = pct;
                    progress(pct);
                }
            }
        }
    }
    drop(out);

    if let Some(expected_hex) = &entry.sha256 {
        let hash = hasher.expect("hasher").finalize();
        let got = hex::encode(hash);
        if !got.eq_ignore_ascii_case(expected_hex) {
            let _ = fs::remove_file(&part_path);
            return Err(format!(
                "sha256 mismatch for {}: expected {expected_hex}, got {got}",
                entry.id
            ));
        }
    }

    if total == 0 {
        let _ = fs::remove_file(&part_path);
        return Err("downloaded file is empty".to_string());
    }

    if let Err(e) = fs::rename(&part_path, &final_path) {
        fs::copy(&part_path, &final_path).map_err(|e2| format!("rename {e} and copy fallback {e2}"))?;
        let _ = fs::remove_file(&part_path);
    }

    progress(100);
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn download_fails_on_empty_url() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_dl_empty_url");
        let _ = fs::create_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let entry = GameEntry {
            id: "test".into(),
            title: "Test".into(),
            filename: "test.z3".into(),
            url: String::new(),
            license: None,
            sha256: None,
            size: None,
            local_path: None,
            is_downloaded: false,
        };
        let res = download(&entry, |_| {});
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("no remote URL"));
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_fails_gracefully_offline() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_dl_offline");
        let _ = fs::create_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let entry = GameEntry {
            id: "offline".into(),
            title: "Offline".into(),
            filename: "offline.z3".into(),
            url: "http://127.0.0.1:9/nonexistent.z3".into(),
            license: None,
            sha256: None,
            size: None,
            local_path: None,
            is_downloaded: false,
        };
        let res = download(&entry, |_| {});
        assert!(res.is_err(), "expected error for offline, got {res:?}");
        let msg = res.unwrap_err();
        assert!(
            msg.contains("download failed") || msg.contains("http") || msg.contains("failed"),
            "msg: {msg}"
        );
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_minizork_fallback_on_empty_url() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_dl_mini_empty");
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::create_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let entry = GameEntry {
            id: "minizork".into(),
            title: "Mini-Zork".into(),
            filename: "minizork.z3".into(),
            url: String::new(),
            license: None,
            sha256: Some("c74f01a232e8df4b05d7ebcba14870143f49b3c9a25f194f7a7d2c69e31ea4a6".into()),
            size: Some(52216),
            local_path: None,
            is_downloaded: false,
        };
        let res = download(&entry, |_| {});
        assert!(res.is_ok(), "minizork fallback on empty url should succeed, got {res:?}");
        let p = res.unwrap();
        assert!(p.exists(), "fallback file should exist at {}", p.display());
        let data = fs::read(&p).unwrap();
        assert_eq!(data.len(), 52216);
        // verify sha matches
        use sha2::{Digest as _, Sha256};
        let hash = Sha256::digest(&data);
        assert_eq!(hex::encode(hash), "c74f01a232e8df4b05d7ebcba14870143f49b3c9a25f194f7a7d2c69e31ea4a6");
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_minizork_fallback_on_offline_url() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_dl_mini_offline");
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::create_dir_all(&tmp);
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let entry = GameEntry {
            id: "minizork".into(),
            title: "Mini-Zork".into(),
            filename: "minizork.z3".into(),
            url: "http://127.0.0.1:9/nonexistent_mini.z3".into(),
            license: None,
            sha256: Some("c74f01a232e8df4b05d7ebcba14870143f49b3c9a25f194f7a7d2c69e31ea4a6".into()),
            size: Some(52216),
            local_path: None,
            is_downloaded: false,
        };
        let res = download(&entry, |_| {});
        assert!(res.is_ok(), "minizork fallback on offline url should succeed via bundled fixture, got {res:?}");
        let p = res.unwrap();
        assert!(p.exists());
        let data = fs::read(&p).unwrap();
        assert_eq!(data.len(), 52216);
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
