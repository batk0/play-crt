#![allow(clippy::pedantic)]

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;

/// Entry as stored in the bundled BASIC JSON manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicManifestEntry {
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

/// Enriched entry exposed to the UI.
#[derive(Debug, Clone)]
pub struct BasicEntry {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub url: String,
    #[allow(dead_code)]
    pub license: Option<String>,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub local_path: Option<PathBuf>,
    pub is_downloaded: bool,
}

/// Return the bundled BASIC manifest path if it exists.
#[must_use]
pub fn basic_manifest_path() -> Option<PathBuf> {
    paths::bundled_basic_manifest_path()
}

/// Load the bundled BASIC manifest. Returns empty vec if missing.
#[must_use]
pub fn load_manifest() -> Vec<BasicManifestEntry> {
    let Some(path) = basic_manifest_path() else {
        if std::env::var("DEBUG").is_ok() {
            eprintln!("basic_catalog: no bundled manifest found (looked in assets/manifests/basic.json)");
        }
        return Vec::new();
    };
    load_manifest_from(&path)
}

fn load_manifest_from(path: &Path) -> Vec<BasicManifestEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        if std::env::var("DEBUG").is_ok() {
            eprintln!("basic_catalog: failed to read {}", path.display());
        }
        return Vec::new();
    };
    match serde_json::from_str::<Vec<BasicManifestEntry>>(&text) {
        Ok(v) => v,
        Err(e) => {
            if std::env::var("DEBUG").is_ok() {
                eprintln!("basic_catalog: failed to parse {}: {e}", path.display());
            }
            Vec::new()
        }
    }
}

/// Discover available BASIC games by merging manifest entries with local files.
#[must_use]
pub fn discover() -> Vec<BasicEntry> {
    let manifest = load_manifest();
    let bdir = paths::basic_dir();
    let local_files = list_local_basics(&bdir);
    build_entries(manifest, &bdir, &local_files)
}

fn build_entries(
    manifest: Vec<BasicManifestEntry>,
    bdir: &Path,
    local_files: &[PathBuf],
) -> Vec<BasicEntry> {
    let mut entries: Vec<BasicEntry> = Vec::new();

    let find_local = |entry: &BasicManifestEntry| -> Option<PathBuf> {
        let cand1 = bdir.join(&entry.id).join(&entry.filename);
        if cand1.exists() {
            return Some(cand1);
        }
        let cand2 = bdir.join(&entry.id).join("game.py");
        if cand2.exists() {
            return Some(cand2);
        }
        let cand3 = bdir.join(&entry.filename);
        if cand3.exists() {
            return Some(cand3);
        }
        for lf in local_files {
            if let Some(name) = lf.file_name().and_then(|s| s.to_str()) {
                if name.eq_ignore_ascii_case(&entry.filename) {
                    return Some(lf.clone());
                }
            }
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
        let local = find_local(&m);
        let is_downloaded = local.is_some();
        if let Some(p) = &local {
            seen_local.insert(p.clone());
        }
        entries.push(BasicEntry {
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

    // Add local-only .py files not covered by manifest
    for lf in local_files {
        if seen_local.contains(lf) {
            continue;
        }
        let matched = entries.iter().any(|e| e.local_path.as_ref() == Some(lf));
        if matched {
            continue;
        }
        let stem = lf
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let id = paths::sanitize_game_id(stem);
        let title = stem.to_string();
        let filename = lf
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&title)
            .to_string();
        entries.push(BasicEntry {
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

fn list_local_basics(bdir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !bdir.exists() {
        return out;
    }
    collect_recursive(bdir, &mut out);
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
        } else if is_basic_file(&p) {
            out.push(p);
        }
    }
}

fn is_basic_file(p: &Path) -> bool {
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ext == "py"
}

/// Return true if python3 is available on PATH. Result is cached after first check.
#[must_use]
pub fn is_python_available() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_manifest_parses() {
        let m = load_manifest();
        assert!(m.len() >= 10, "basic manifest len {}", m.len());
        assert!(m.iter().any(|e| e.id == "acey_ducey"));
    }

    #[test]
    fn discover_with_tmp_data_dir() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_basic_catalog_discover_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("basic").join("acey_ducey")).unwrap();
        fs::write(tmp.join("basic").join("acey_ducey").join("acey_ducey.py"), b"print('hi')").unwrap();
        fs::write(tmp.join("basic").join("mygame.py"), b"print('x')").unwrap();

        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let entries = discover();
        let acey = entries.iter().find(|e| e.id == "acey_ducey").expect("acey_ducey");
        assert!(acey.is_downloaded, "acey_ducey should be downloaded");
        assert!(acey.local_path.is_some());
        assert!(entries.iter().any(|e| e.local_path.as_ref().map(|p| p.ends_with("mygame.py")).unwrap_or(false)));

        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_empty_when_no_manifest_and_no_local() {
        let tmp = std::env::temp_dir().join("play_crt_basic_catalog_empty");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let e = build_entries(Vec::new(), &tmp, &[]);
        assert!(e.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }
}
