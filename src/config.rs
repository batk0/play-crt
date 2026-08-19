//! Persistence for bezel switches + catalog choice.
//!
//! Stores `ControlState` + last `CatalogKind` in `data_dir/config.json` as
//! JSON so settings survive restart. Handles missing / corrupted files
//! gracefully by falling back to defaults.

#![allow(clippy::pedantic)]

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::controls::{BaudRate, ControlState, PhosphorColor, SoundPalette};
use crate::menu::CatalogKind;
use crate::paths;

// ── Serde defaults ───────────────────────────────────────────────────────

fn default_phosphor() -> String {
    "Green".to_string()
}

fn default_true() -> bool {
    true
}

fn default_baud() -> String {
    "2400".to_string()
}

fn default_sound() -> String {
    "Teletype".to_string()
}

// ── Config struct ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default = "default_phosphor")]
    pub phosphor: String,
    #[serde(default = "default_true")]
    pub curvature: bool,
    #[serde(default = "default_true")]
    pub flicker: bool,
    #[serde(default = "default_true")]
    pub scanlines: bool,
    #[serde(default = "default_baud")]
    pub baud: String,
    #[serde(default = "default_sound")]
    pub sound: String,
    #[serde(default)]
    pub last_catalog: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            phosphor: default_phosphor(),
            curvature: true,
            flicker: true,
            scanlines: true,
            baud: default_baud(),
            sound: default_sound(),
            last_catalog: None,
        }
    }
}

impl Config {
    #[allow(dead_code)]
    #[must_use]
    pub fn from_control_state(state: &ControlState) -> Self {
        Self {
            phosphor: state.phosphor.as_config_str().to_string(),
            curvature: state.curvature_enabled,
            flicker: state.flicker_enabled,
            scanlines: state.scanlines_enabled,
            baud: state.baud_rate.as_config_str().to_string(),
            sound: state.sound_palette.as_config_str().to_string(),
            last_catalog: None,
        }
    }

    #[must_use]
    pub fn to_control_state(&self) -> ControlState {
        ControlState {
            phosphor: PhosphorColor::from_config_str(&self.phosphor),
            curvature_enabled: self.curvature,
            flicker_enabled: self.flicker,
            scanlines_enabled: self.scanlines,
            baud_rate: BaudRate::from_config_str(&self.baud),
            sound_palette: SoundPalette::from_config_str(&self.sound),
        }
    }

    /// Update the control fields from `state`, preserving `last_catalog`.
    pub fn apply_control_state(&mut self, state: &ControlState) {
        self.phosphor = state.phosphor.as_config_str().to_string();
        self.curvature = state.curvature_enabled;
        self.flicker = state.flicker_enabled;
        self.scanlines = state.scanlines_enabled;
        self.baud = state.baud_rate.as_config_str().to_string();
        self.sound = state.sound_palette.as_config_str().to_string();
    }
}

// ── Path ─────────────────────────────────────────────────────────────────

#[must_use]
pub fn config_path() -> PathBuf {
    paths::data_dir().join("config.json")
}

// ── Load / Save ──────────────────────────────────────────────────────────

/// Load config from disk. Returns defaults on missing / corrupted file.
#[must_use]
pub fn load() -> Config {
    let path = config_path();
    let Ok(bytes) = fs::read(&path) else {
        return Config::default();
    };
    serde_json::from_slice::<Config>(&bytes).unwrap_or_default()
}

/// Atomically write `cfg` to `config_path()`.
///
/// Writes to `<path>.tmp` then renames. Ensures parent dir exists.
pub fn save(cfg: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config dir {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize config: {e}"))?;
    // Write to temp file in same directory so rename is atomic on same filesystem.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json.as_bytes()).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

// ── ControlState helpers ─────────────────────────────────────────────────

/// Load `ControlState` from persisted config (or defaults).
#[must_use]
pub fn load_control_state() -> ControlState {
    load().to_control_state()
}

/// Persist `state`, preserving any existing `last_catalog`.
pub fn save_control_state(state: &ControlState) -> Result<(), String> {
    let mut cfg = load();
    cfg.apply_control_state(state);
    save(&cfg)
}

// ── CatalogKind helpers ──────────────────────────────────────────────────

#[must_use]
pub fn catalog_kind_to_str(kind: CatalogKind) -> &'static str {
    match kind {
        CatalogKind::ZMachine => "zmachine",
        CatalogKind::Basic => "basic",
    }
}

#[must_use]
pub fn catalog_kind_from_str(s: &str) -> Option<CatalogKind> {
    match s.trim().to_ascii_lowercase().as_str() {
        "zmachine" | "z-machine" | "z_machine" => Some(CatalogKind::ZMachine),
        "basic" | "basic_games" | "basic games" => Some(CatalogKind::Basic),
        _ => None,
    }
}

/// Load last catalog kind if persisted.
#[must_use]
pub fn load_last_catalog() -> Option<CatalogKind> {
    let cfg = load();
    cfg.last_catalog.as_deref().and_then(catalog_kind_from_str)
}

/// Persist last catalog kind, preserving control fields.
pub fn save_last_catalog(kind: CatalogKind) -> Result<(), String> {
    let mut cfg = load();
    cfg.last_catalog = Some(catalog_kind_to_str(kind).to_string());
    save(&cfg)
}

/// Persist both control state and catalog kind atomically in one write.
#[allow(dead_code)]
pub fn save_all(state: &ControlState, kind: Option<CatalogKind>) -> Result<(), String> {
    let mut cfg = load();
    cfg.apply_control_state(state);
    if let Some(k) = kind {
        cfg.last_catalog = Some(catalog_kind_to_str(k).to_string());
    }
    save(&cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::PhosphorColor;
    use std::fs;

    fn with_tmp_dir<F: FnOnce()>(f: F) {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "play_crt_config_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        f();
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn defaults_are_green_all_true() {
        let cfg = Config::default();
        assert_eq!(cfg.phosphor, "Green");
        assert!(cfg.curvature);
        assert!(cfg.flicker);
        assert!(cfg.scanlines);
        assert_eq!(cfg.baud, "2400");
        assert_eq!(cfg.sound, "Teletype");
        assert!(cfg.last_catalog.is_none());
        let state = cfg.to_control_state();
        assert_eq!(state.phosphor, PhosphorColor::Green);
        assert!(state.curvature_enabled);
        assert_eq!(state.baud_rate, crate::controls::BaudRate::Baud2400);
        assert_eq!(state.sound_palette, crate::controls::SoundPalette::Teletype);
    }

    #[test]
    fn phosphor_roundtrip() {
        for (color, s) in [
            (PhosphorColor::Green, "Green"),
            (PhosphorColor::Amber, "Amber"),
            (PhosphorColor::White, "White"),
        ] {
            assert_eq!(color.as_config_str(), s);
            assert_eq!(PhosphorColor::from_config_str(s), color);
            // case-insensitive
            assert_eq!(PhosphorColor::from_config_str(&s.to_ascii_lowercase()), color);
            assert_eq!(PhosphorColor::from_config_str(&s.to_ascii_uppercase()), color);
        }
        // unknown → Green
        assert_eq!(PhosphorColor::from_config_str("unknown"), PhosphorColor::Green);
        assert_eq!(PhosphorColor::from_config_str(""), PhosphorColor::Green);
    }

    #[test]
    fn config_path_uses_data_dir() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_config_path_test");
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let p = config_path();
        assert!(p.ends_with("config.json"));
        assert_eq!(p.parent().unwrap(), tmp);
        if let Some(v) = prev {
            std::env::set_var("PLAY_CRT_DATA_DIR", v);
        } else {
            std::env::remove_var("PLAY_CRT_DATA_DIR");
        }
    }

    #[test]
    fn load_missing_returns_default() {
        with_tmp_dir(|| {
            // no file yet
            let cfg = load();
            assert_eq!(cfg, Config::default());
        });
    }

    #[test]
    fn save_and_load_roundtrip() {
        with_tmp_dir(|| {
            let cfg = Config {
                phosphor: "Amber".to_string(),
                curvature: false,
                flicker: false,
                scanlines: true,
                baud: "9600".to_string(),
                sound: "Minimal".to_string(),
                last_catalog: Some("basic".to_string()),
            };
            save(&cfg).expect("save");
            // file exists
            assert!(config_path().exists());
            // no tmp left
            assert!(!config_path().with_extension("json.tmp").exists());
            let loaded = load();
            assert_eq!(loaded, cfg);
            let state = loaded.to_control_state();
            assert_eq!(state.phosphor, PhosphorColor::Amber);
            assert!(!state.curvature_enabled);
            assert!(!state.flicker_enabled);
            assert!(state.scanlines_enabled);
            assert_eq!(state.baud_rate, crate::controls::BaudRate::Baud9600);
            assert_eq!(state.sound_palette, crate::controls::SoundPalette::Minimal);
        });
    }

    #[test]
    fn save_control_state_preserves_last_catalog() {
        with_tmp_dir(|| {
            let cfg = Config {
                last_catalog: Some("basic".to_string()),
                ..Default::default()
            };
            save(&cfg).unwrap();
            let state = ControlState {
                phosphor: PhosphorColor::White,
                curvature_enabled: false,
                flicker_enabled: true,
                scanlines_enabled: false,
                baud_rate: crate::controls::BaudRate::Baud300,
                sound_palette: crate::controls::SoundPalette::ModemCrt,
            };
            save_control_state(&state).unwrap();
            let loaded = load();
            assert_eq!(loaded.phosphor, "White");
            assert!(!loaded.curvature);
            assert_eq!(loaded.baud, "300");
            assert_eq!(loaded.sound, "ModemCrt");
            assert_eq!(loaded.last_catalog, Some("basic".to_string()));
        });
    }

    #[test]
    fn save_last_catalog_preserves_controls() {
        with_tmp_dir(|| {
            let state = ControlState {
                phosphor: PhosphorColor::Amber,
                curvature_enabled: false,
                flicker_enabled: false,
                scanlines_enabled: true,
                baud_rate: crate::controls::BaudRate::Baud1200,
                sound_palette: crate::controls::SoundPalette::Minimal,
            };
            save_control_state(&state).unwrap();
            save_last_catalog(CatalogKind::Basic).unwrap();
            let loaded = load();
            assert_eq!(loaded.phosphor, "Amber");
            assert_eq!(loaded.baud, "1200");
            assert_eq!(loaded.sound, "Minimal");
            assert_eq!(loaded.last_catalog, Some("basic".to_string()));
            assert_eq!(load_last_catalog(), Some(CatalogKind::Basic));
            // switch back
            save_last_catalog(CatalogKind::ZMachine).unwrap();
            assert_eq!(load_last_catalog(), Some(CatalogKind::ZMachine));
        });
    }

    #[test]
    fn corrupted_json_returns_default() {
        with_tmp_dir(|| {
            let path = config_path();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"{ not valid json").unwrap();
            let cfg = load();
            assert_eq!(cfg, Config::default());
            // partial JSON with missing fields should fill defaults
            fs::write(&path, br#"{"phosphor":"White"}"#).unwrap();
            let cfg2 = load();
            assert_eq!(cfg2.phosphor, "White");
            // missing bools default to true via serde(default)
            assert!(cfg2.curvature);
            assert!(cfg2.flicker);
            assert!(cfg2.scanlines);
            assert_eq!(cfg2.baud, "2400");
            assert_eq!(cfg2.sound, "Teletype");
        });
    }

    #[test]
    fn atomic_write_no_tmp_leak() {
        with_tmp_dir(|| {
            let state = ControlState {
                phosphor: PhosphorColor::Green,
                curvature_enabled: true,
                flicker_enabled: true,
                scanlines_enabled: true,
                baud_rate: crate::controls::BaudRate::Baud2400,
                sound_palette: crate::controls::SoundPalette::Teletype,
            };
            save_control_state(&state).unwrap();
            save_control_state(&state).unwrap();
            assert!(!config_path().with_extension("json.tmp").exists());
        });
    }

    #[test]
    fn catalog_kind_string_helpers() {
        assert_eq!(catalog_kind_to_str(CatalogKind::ZMachine), "zmachine");
        assert_eq!(catalog_kind_to_str(CatalogKind::Basic), "basic");
        assert_eq!(catalog_kind_from_str("zmachine"), Some(CatalogKind::ZMachine));
        assert_eq!(catalog_kind_from_str("Z-Machine"), Some(CatalogKind::ZMachine));
        assert_eq!(catalog_kind_from_str("BASIC"), Some(CatalogKind::Basic));
        assert_eq!(catalog_kind_from_str("unknown"), None);
    }
}
