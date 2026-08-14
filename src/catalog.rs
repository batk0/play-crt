#![allow(clippy::pedantic)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;

/// Embedded Mini-Zork fixture — same bytes as `src/zmachine/fixtures/minizork.z3`.
/// Used to mark minizork as locally available without a network download.
const MINIZORK_BYTES: &[u8] = include_bytes!("zmachine/fixtures/minizork.z3");

/// Try to ensure `stories/minizork/<filename>` exists from the bundled fixture.
/// Returns the installed path on success. Only applies to minizork.
fn ensure_minizork_installed(sdir: &Path, entry: &ManifestEntry) -> Option<PathBuf> {
    if entry.id != "minizork" {
        return None;
    }
    let dest_dir = sdir.join(&entry.id);
    let dest = dest_dir.join(&entry.filename);
    if dest.exists() {
        return Some(dest);
    }
    // Try embedded bytes first
    if fs::create_dir_all(&dest_dir).is_ok() && fs::write(&dest, MINIZORK_BYTES).is_ok() {
        return Some(dest);
    }
    // Fallback: look for filesystem fixture in dev tree
    if let Some(src) = find_filesystem_minizork_fixture() {
        if fs::create_dir_all(&dest_dir).is_ok() && fs::copy(&src, &dest).is_ok() {
            return Some(dest);
        }
    }
    None
}

fn find_filesystem_minizork_fixture() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("src/zmachine/fixtures/minizork.z3")];
    if let Ok(cwd) = std::env::current_dir() {
        for anc in cwd.ancestors() {
            candidates.push(anc.join("src/zmachine/fixtures/minizork.z3"));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("src/zmachine/fixtures/minizork.z3"));
            candidates.push(parent.join("../src/zmachine/fixtures/minizork.z3"));
            candidates.push(parent.join("../../src/zmachine/fixtures/minizork.z3"));
        }
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Return a bundled fixture path to use as `local_path` without copying,
/// if no installed copy exists but a dev-tree fixture is present.
fn bundled_minizork_local_path() -> Option<PathBuf> {
    find_filesystem_minizork_fixture()
}

/// Entry as stored in the bundled JSON manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub url: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

/// Enriched entry exposed to the UI — merges manifest + local discovery.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GameEntry {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub url: String,
    pub license: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub local_path: Option<PathBuf>,
    pub is_downloaded: bool,
}

/// Return the bundled manifest path if it exists (search assets/manifests/…).
#[must_use]
pub fn stories_manifest_path() -> Option<PathBuf> {
    crate::paths::bundled_manifest_path()
}

/// Load the bundled manifest. Returns empty vec if missing (offline dev).
pub fn load_manifest() -> Vec<ManifestEntry> {
    let Some(path) = stories_manifest_path() else {
        eprintln!("catalog: no bundled manifest found (looked in assets/manifests/stories.json)");
        return Vec::new();
    };
    load_manifest_from(&path)
}

fn load_manifest_from(path: &Path) -> Vec<ManifestEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        eprintln!("catalog: failed to read {}", path.display());
        return Vec::new();
    };
    match serde_json::from_str::<Vec<ManifestEntry>>(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("catalog: failed to parse {}: {e}", path.display());
            Vec::new()
        }
    }
}

/// Discover available games by merging manifest entries with local files.
///
/// - For each manifest entry, `is_downloaded` is true when
///   `stories_dir/<id>/<filename>` or `stories_dir/<id>/game.z3` or
///   `stories_dir/<filename>` exists, or any local file with matching id/filename.
/// - Any `.z3/.z5/.z8/.zip` found under `stories_dir` that does not correspond
///   to a manifest entry is added as an extra `GameEntry` (local-only).
#[must_use]
pub fn discover() -> Vec<GameEntry> {
    let manifest = load_manifest();
    let sdir = paths::stories_dir();
    let local_files = list_local_stories(&sdir);
    build_entries(manifest, &sdir, &local_files)
}

fn build_entries(
    manifest: Vec<ManifestEntry>,
    sdir: &Path,
    local_files: &[PathBuf],
) -> Vec<GameEntry> {
    // Index local files by sanitized stem/id and by filename
    let mut entries: Vec<GameEntry> = Vec::new();

    // Helper to find local_path for a manifest entry
    let find_local = |entry: &ManifestEntry| -> Option<PathBuf> {
        // Preferred: stories/<id>/<filename>
        let cand1 = sdir.join(&entry.id).join(&entry.filename);
        if cand1.exists() {
            return Some(cand1);
        }
        // Alternate: stories/<id>/game.z3 (spec atomic rename target)
        let cand2 = sdir.join(&entry.id).join("game.z3");
        if cand2.exists() {
            return Some(cand2);
        }
        // Legacy flat: stories/<filename>
        let cand3 = sdir.join(&entry.filename);
        if cand3.exists() {
            return Some(cand3);
        }
        // Search among local_files for filename match
        for lf in local_files {
            if let Some(name) = lf.file_name().and_then(|s| s.to_str()) {
                if name.eq_ignore_ascii_case(&entry.filename) {
                    return Some(lf.clone());
                }
            }
            // Also allow id stem match
            if let Some(stem) = lf.file_stem().and_then(|s| s.to_str()) {
                if paths::sanitize_game_id(stem) == entry.id {
                    return Some(lf.clone());
                }
            }
        }
        None
    };

    let mut seen_local: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for m in manifest {
        let mut local = find_local(&m);
        // Mini-Zork fallback: treat bundled fixture as local so the UI shows [Ready]
        // even before any download. Try to auto-install embedded bytes to stories/minizork/
        // so subsequent launches are self-contained.
        if local.is_none() && m.id == "minizork" {
            if let Some(installed) = ensure_minizork_installed(sdir, &m) {
                local = Some(installed);
            } else if let Some(bundled) = bundled_minizork_local_path() {
                local = Some(bundled);
            }
        }
        let is_downloaded = local.is_some();
        if let Some(p) = &local {
            seen_local.insert(p.clone());
        }
        entries.push(GameEntry {
            id: m.id,
            title: m.title,
            filename: m.filename,
            url: m.url,
            license: m.license,
            sha256: m.sha256,
            size: m.size,
            local_path: local,
            is_downloaded,
        });
    }

    // Add local-only files not covered by manifest
    for lf in local_files {
        if seen_local.contains(lf) {
            continue;
        }
        // Check if this file was already matched by id/filename above via different cand
        let matched = entries.iter().any(|e| e.local_path.as_ref() == Some(lf));
        if matched {
            continue;
        }
        let stem = lf
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let id = paths::game_id_for_path(lf);
        let title = stem.to_string();
        let filename = lf
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&title)
            .to_string();
        entries.push(GameEntry {
            id,
            title,
            filename,
            url: String::new(),
            license: None,
            sha256: None,
            size: None,
            local_path: Some(lf.clone()),
            is_downloaded: true,
        });
    }

    entries
}

fn list_local_stories(sdir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !sdir.exists() {
        return out;
    }
    collect_recursive(sdir, &mut out);
    out.sort();
    out
}

fn collect_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for ent in entries.flatten() {
        let p = ent.path();
        if p.is_dir() {
            collect_recursive(&p, out);
        } else if is_story_file(&p) {
            out.push(p);
        }
    }
}

fn is_story_file(p: &Path) -> bool {
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "z3" | "z5" | "z8" | "z1" | "z2" | "z4" | "zip" | "zblorb" | "ulx")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_manifest_parses() {
        let m = load_manifest();
        // Bundled manifest should have at least the zorks
        assert!(m.len() >= 4, "manifest len {}", m.len());
        assert!(m.iter().any(|e| e.id == "zork1"));
        assert!(m.iter().any(|e| e.id == "minizork"));
    }

    #[test]
    fn discover_with_tmp_data_dir() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_catalog_discover_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("stories").join("zork1")).unwrap();
        // Simulate downloaded zork1
        fs::write(tmp.join("stories").join("zork1").join("zork1.z3"), b"FAKEZ3").unwrap();
        // Also local-only file
        fs::write(tmp.join("stories").join("mygame.z5"), b"FAKE").unwrap();

        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let entries = discover();
        // zork1 should be marked downloaded
        let z1 = entries.iter().find(|e| e.id == "zork1").expect("zork1");
        assert!(z1.is_downloaded, "zork1 should be downloaded");
        assert!(z1.local_path.is_some());
        // local-only file should appear
        assert!(entries.iter().any(|e| e.local_path.as_ref().map(|p| p.ends_with("mygame.z5")).unwrap_or(false)));

        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_empty_when_no_manifest_and_no_local() {
        // Use a tmp dir with no stories and override manifest search by using build_entries directly
        let tmp = std::env::temp_dir().join("play_crt_catalog_empty");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        // build_entries with empty manifest should give only locals (none)
        let e = build_entries(Vec::new(), &tmp, &[]);
        assert!(e.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_minizork_bundled_fallback() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_catalog_mini_fallback");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        // No local stories initially — discover should auto-install bundled minizork
        let entries = discover();
        let mini = entries.iter().find(|e| e.id == "minizork").expect("minizork entry");
        assert!(mini.is_downloaded, "minizork should be marked downloaded via bundled fallback");
        assert!(mini.local_path.is_some(), "minizork should have local_path via fallback");
        let p = mini.local_path.as_ref().unwrap();
        assert!(p.exists(), "bundled fallback path should exist: {}", p.display());
        let data = fs::read(p).unwrap();
        assert_eq!(data.len(), MINIZORK_BYTES.len());
        assert_eq!(data, MINIZORK_BYTES);
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
