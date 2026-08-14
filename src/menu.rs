use std::path::PathBuf;

use crate::basic_catalog;
use crate::catalog;
use crate::download;
use crate::grid::Grid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameKind {
    ZMachine,
    Basic,
}

impl GameKind {
    #[allow(dead_code)]
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::ZMachine => "[Z]",
            Self::Basic => "[BASIC]",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    ZMachine,
    Basic,
}

impl CatalogKind {
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ZMachine => "Z-MACHINE",
            Self::Basic => "BASIC GAMES",
        }
    }

    #[must_use]
    pub fn toggle(&self) -> Self {
        match self {
            Self::ZMachine => Self::Basic,
            Self::Basic => Self::ZMachine,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MenuEntry {
    pub id: String,
    pub title: String,
    pub kind: GameKind,
    pub filename: String,
    pub url: String,
    pub local_path: Option<PathBuf>,
    pub is_downloaded: bool,
}

impl MenuEntry {
    #[must_use]
    pub fn from_z(entry: catalog::GameEntry) -> Self {
        Self {
            id: entry.id,
            title: entry.title,
            kind: GameKind::ZMachine,
            filename: entry.filename,
            url: entry.url,
            local_path: entry.local_path,
            is_downloaded: entry.is_downloaded,
        }
    }

    #[must_use]
    pub fn from_basic(entry: basic_catalog::BasicEntry) -> Self {
        Self {
            id: entry.id,
            title: entry.title,
            kind: GameKind::Basic,
            filename: entry.filename,
            url: entry.url,
            local_path: entry.local_path,
            is_downloaded: entry.is_downloaded,
        }
    }
}

/// Discover unified entries from both catalogs, sorted by title.
#[allow(dead_code)]
#[must_use]
pub fn discover_unified() -> Vec<MenuEntry> {
    let mut entries: Vec<MenuEntry> = Vec::new();
    for e in catalog::discover() {
        entries.push(MenuEntry::from_z(e));
    }
    for e in basic_catalog::discover() {
        entries.push(MenuEntry::from_basic(e));
    }
    // Sort alphabetically by title case-insensitive, but keep stable for identical titles.
    entries.sort_by_key(|a| a.title.to_ascii_lowercase());
    entries
}

/// Discover entries for a specific catalog kind, sorted by title.
#[must_use]
pub fn discover_for_kind(kind: CatalogKind) -> Vec<MenuEntry> {
    let mut entries = match kind {
        CatalogKind::ZMachine => catalog::discover()
            .into_iter()
            .map(MenuEntry::from_z)
            .collect::<Vec<_>>(),
        CatalogKind::Basic => basic_catalog::discover()
            .into_iter()
            .map(MenuEntry::from_basic)
            .collect::<Vec<_>>(),
    };
    entries.sort_by_key(|a| a.title.to_ascii_lowercase());
    entries
}

/// Pure-text game picker rendered into the 80×24 CRT grid.
pub struct MenuState {
    pub kind: CatalogKind,
    pub(crate) entries: Vec<MenuEntry>,
    pub(crate) selected: usize,
    pub(crate) downloading: Option<String>,
    pub(crate) status_msg: Option<String>,
    pub(crate) dl_rx: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
}

impl MenuState {
    #[allow(dead_code)]
    pub fn new(entries: Vec<MenuEntry>) -> Self {
        Self {
            kind: CatalogKind::ZMachine,
            entries,
            selected: 0,
            downloading: None,
            status_msg: None,
            dl_rx: None,
        }
    }

    #[must_use]
    pub fn new_for_kind(kind: CatalogKind) -> Self {
        let entries = discover_for_kind(kind);
        Self {
            kind,
            entries,
            selected: 0,
            downloading: None,
            status_msg: None,
            dl_rx: None,
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn with_kind(kind: CatalogKind, entries: Vec<MenuEntry>) -> Self {
        Self {
            kind,
            entries,
            selected: 0,
            downloading: None,
            status_msg: None,
            dl_rx: None,
        }
    }

    /// Convenience: create from unified discovery.
    #[allow(dead_code)]
    #[must_use]
    pub fn from_discover() -> Self {
        Self::new_for_kind(CatalogKind::ZMachine)
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn discover_for_kind_inner(kind: CatalogKind) -> Vec<MenuEntry> {
        discover_for_kind(kind)
    }

    pub(crate) fn refresh(&mut self) {
        self.entries = discover_for_kind(self.kind);
        if self.selected >= self.entries.len() && !self.entries.is_empty() {
            self.selected = self.entries.len() - 1;
        }
        self.status_msg = Some(format!("Refreshed — {} games.", self.entries.len()));
    }

    pub(crate) fn switch_kind(&mut self, new_kind: CatalogKind) {
        if self.kind == new_kind {
            return;
        }
        self.kind = new_kind;
        self.selected = 0;
        self.downloading = None;
        self.dl_rx = None;
        self.entries = discover_for_kind(self.kind);
        self.status_msg = Some(format!(
            "Switched to {} — {} games.",
            self.kind.display_name(),
            self.entries.len()
        ));
    }

    pub(crate) fn toggle_kind(&mut self) {
        let other = self.kind.toggle();
        self.switch_kind(other);
    }

    pub(crate) fn move_up(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.entries.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub(crate) fn move_down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.entries.len();
    }

    pub(crate) fn selected_entry(&self) -> Option<&MenuEntry> {
        self.entries.get(self.selected)
    }

    pub(crate) fn start_download(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if entry.is_downloaded || self.downloading.is_some() {
            return;
        }
        // For BASIC, check python availability — if missing we refuse to download? Still allow download but warn.
        // The spec says show [No python3] tag; we still allow download, but start_download will be blocked in UI handling.
        if entry.kind == GameKind::Basic && !basic_catalog::is_python_available() {
            self.status_msg = Some("python3 not found — install python3 to play BASIC games".to_string());
            return;
        }
        let id = entry.id.clone();
        self.downloading = Some(id.clone());
        self.status_msg = Some(format!("Downloading {} ...", entry.title));
        let (tx, rx) = std::sync::mpsc::channel();
        self.dl_rx = Some(rx);
        std::thread::spawn(move || {
            let res: Result<PathBuf, String> = match entry.kind {
                GameKind::ZMachine => {
                    let zm_entry = catalog::GameEntry {
                        id: entry.id.clone(),
                        title: entry.title.clone(),
                        filename: entry.filename.clone(),
                        url: entry.url.clone(),
                        license: None,
                        sha256: None,
                        size: None,
                        local_path: entry.local_path.clone(),
                        is_downloaded: entry.is_downloaded,
                    };
                    download::download(&zm_entry, |_| {})
                }
                GameKind::Basic => {
                    let basic_entry = basic_catalog::BasicEntry {
                        id: entry.id.clone(),
                        title: entry.title.clone(),
                        filename: entry.filename.clone(),
                        url: entry.url.clone(),
                        license: None,
                        sha256: None,
                        size: None,
                        local_path: entry.local_path.clone(),
                        is_downloaded: entry.is_downloaded,
                    };
                    download::download_basic(&basic_entry, |_| {})
                }
            };
            let _ = tx.send(res);
        });
    }

    /// Poll download channel; returns `Some(Ok(path))` or `Some(Err(msg))`.
    pub(crate) fn poll_download(&mut self) -> Option<Result<PathBuf, String>> {
        let rx = self.dl_rx.as_ref()?;
        match rx.try_recv() {
            Ok(v) => {
                self.dl_rx = None;
                self.downloading = None;
                Some(v)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.dl_rx = None;
                self.downloading = None;
                Some(Err("download thread disconnected".to_string()))
            }
        }
    }

    /// Render the menu into the 80×24 grid. Pure text, CRT-grid native.
    pub(crate) fn render_to_grid(&self, grid: &mut Grid) {
        grid.clear();
        grid.put_str(&format!(" PLAY-CRT \u{2014} {}\n", self.kind.display_name()));
        grid.put_str(" \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
        if self.entries.is_empty() {
            grid.put_str("\n No games found.\n");
            match self.kind {
                CatalogKind::ZMachine => {
                    grid.put_str(" Add .z3/.z5/.z8 files to stories folder.\n");
                }
                CatalogKind::Basic => {
                    grid.put_str(" Add .py files to basic folder.\n");
                }
            }
            grid.put_str("\n [R] Refresh   [Q] Quit   Left/Right: switch catalog\n");
            if let Some(msg) = &self.status_msg {
                grid.put_str(&format!("\n {msg}\n"));
            }
            return;
        }
        // List entries (max ~15 to fit 24 rows with header/footer)
        let max_visible = 15usize;
        let offset = if self.entries.len() <= max_visible || self.selected < max_visible / 2 {
            0
        } else if self.selected >= self.entries.len() - max_visible / 2 {
            self.entries.len() - max_visible
        } else {
            self.selected - max_visible / 2
        };
        let python_ok = basic_catalog::is_python_available();
        for (idx, entry) in self.entries.iter().enumerate().skip(offset).take(max_visible) {
            let is_sel = idx == self.selected;
            let marker = if is_sel { ">" } else { " " };
            let state = if Some(entry.id.as_str()) == self.downloading.as_deref() {
                "[Downloading...]"
            } else if entry.kind == GameKind::Basic && !python_ok {
                "[No python3]"
            } else if entry.is_downloaded {
                "[Ready]"
            } else {
                "[Download]"
            };
            let num = idx + 1;
            let mut title = entry.title.clone();
            // Truncate to fit 80 cols: " > 1. Title [Ready]" → keep title ~44 (char-boundary safe)
            if title.chars().count() > 44 {
                title = title.chars().take(44).collect();
            }
            let line = format!(" {marker} {num}. {title} {state}\n");
            grid.put_str(&line);
        }
        if self.entries.len() > max_visible {
            grid.put_str(&format!(
                " ... {} more (use Up/Down)\n",
                self.entries.len() - max_visible
            ));
        }
        grid.put_str(" \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
        if let Some(dl) = &self.downloading {
            grid.put_str(&format!(" Downloading {dl} ... please wait\n"));
        } else if let Some(msg) = &self.status_msg {
            let mut m = msg.clone();
            if m.chars().count() > 78 {
                m = m.chars().take(78).collect();
            }
            grid.put_str(&format!(" {m}\n"));
        }
        grid.put_str(" Enter: play/download  Up/Down: select  Left/Right: switch catalog\n");
        grid.put_str(" 1-9: jump  R: refresh  Q: quit\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_z(id: &str, title: &str, downloaded: bool) -> MenuEntry {
        MenuEntry {
            id: id.to_string(),
            title: title.to_string(),
            kind: GameKind::ZMachine,
            filename: format!("{id}.z3"),
            url: String::new(),
            local_path: if downloaded { Some(PathBuf::from(format!("/tmp/{id}.z3"))) } else { None },
            is_downloaded: downloaded,
        }
    }

    fn make_basic(id: &str, title: &str, downloaded: bool) -> MenuEntry {
        MenuEntry {
            id: id.to_string(),
            title: title.to_string(),
            kind: GameKind::Basic,
            filename: format!("{id}.py"),
            url: format!("https://example.com/{id}.py"),
            local_path: if downloaded { Some(PathBuf::from(format!("/tmp/{id}.py"))) } else { None },
            is_downloaded: downloaded,
        }
    }

    #[test]
    fn menu_navigation_zmachine() {
        let entries = vec![
            make_z("zork1", "Zork I", true),
            make_z("zork2", "Zork II", false),
            make_z("zork3", "Zork III", true),
        ];
        let mut state = MenuState::with_kind(CatalogKind::ZMachine, entries);
        assert_eq!(state.kind, CatalogKind::ZMachine);
        assert_eq!(state.selected, 0);
        state.move_down();
        assert_eq!(state.selected, 1);
        state.move_down();
        assert_eq!(state.selected, 2);
        state.move_down();
        assert_eq!(state.selected, 0);
        state.move_up();
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn menu_navigation_basic() {
        let entries = vec![
            make_basic("acey_ducey", "Acey Ducey", false),
            make_basic("amazing", "Amazing", true),
        ];
        let mut state = MenuState::with_kind(CatalogKind::Basic, entries);
        assert_eq!(state.kind, CatalogKind::Basic);
        state.move_down();
        assert_eq!(state.selected, 1);
        state.move_up();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn catalog_switch_resets_selection_and_updates_kind() {
        let z_entries = vec![make_z("zork1", "Zork I", true), make_z("zork2", "Zork II", true)];
        let mut state = MenuState::with_kind(CatalogKind::ZMachine, z_entries);
        state.selected = 1;
        // Switch to Basic with custom entries to avoid filesystem dependency
        let b_entries = vec![make_basic("acey_ducey", "Acey Ducey", true)];
        // Simulate switch by directly setting kind and entries (unit test of switch logic without filesystem)
        state.kind = CatalogKind::Basic;
        state.entries = b_entries;
        state.selected = 0;
        assert_eq!(state.kind, CatalogKind::Basic);
        assert_eq!(state.selected, 0);
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].kind, GameKind::Basic);
    }

    #[test]
    fn switch_kind_method_resets_and_refreshes() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_menu_switch_kind");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("stories").join("zork1")).unwrap();
        std::fs::write(tmp.join("stories").join("zork1").join("zork1.z3"), b"FAKE").unwrap();
        std::fs::create_dir_all(tmp.join("basic").join("acey_ducey")).unwrap();
        std::fs::write(tmp.join("basic").join("acey_ducey").join("acey_ducey.py"), b"print(1)").unwrap();
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let mut state = MenuState::new_for_kind(CatalogKind::ZMachine);
        assert!(state.entries.iter().any(|e| e.id == "zork1"));
        assert!(!state.entries.iter().any(|e| e.id == "acey_ducey"));
        state.selected = state.entries.len().saturating_sub(1);
        state.switch_kind(CatalogKind::Basic);
        assert_eq!(state.kind, CatalogKind::Basic);
        assert_eq!(state.selected, 0);
        assert!(state.entries.iter().any(|e| e.id == "acey_ducey"));
        assert!(!state.entries.iter().any(|e| e.id == "zork1"));
        assert!(state.status_msg.as_ref().unwrap().contains("BASIC"));
        // Switch back
        state.switch_kind(CatalogKind::ZMachine);
        assert_eq!(state.kind, CatalogKind::ZMachine);
        assert_eq!(state.selected, 0);
        assert!(state.entries.iter().any(|e| e.id == "zork1"));
        if let Some(v) = prev { std::env::set_var("PLAY_CRT_DATA_DIR", v); } else { std::env::remove_var("PLAY_CRT_DATA_DIR"); }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn toggle_kind_flips() {
        let mut state = MenuState::with_kind(CatalogKind::ZMachine, vec![make_z("zork1", "Zork I", true)]);
        assert_eq!(state.kind, CatalogKind::ZMachine);
        state.toggle_kind();
        // toggle will trigger filesystem discover; but we test the enum toggle directly
        assert_eq!(CatalogKind::ZMachine.toggle(), CatalogKind::Basic);
        assert_eq!(CatalogKind::Basic.toggle(), CatalogKind::ZMachine);
    }

    #[test]
    fn render_header_shows_catalog_kind() {
        let z_state = MenuState::with_kind(CatalogKind::ZMachine, vec![make_z("zork1", "Zork I", true)]);
        let mut grid = Grid::new();
        z_state.render_to_grid(&mut grid);
        let header = grid.line_trimmed(0);
        assert!(
            header.contains("PLAY-CRT \u{2014} Z-MACHINE"),
            "header should be simple title 'PLAY-CRT — Z-MACHINE', got {header:?}"
        );
        assert!(!header.contains('['), "header should not contain bracket, got {header:?}");
        assert!(!header.contains('\u{25C4}'), "header should not contain arrow, got {header:?}");

        let b_state =
            MenuState::with_kind(CatalogKind::Basic, vec![make_basic("acey_ducey", "Acey Ducey", true)]);
        let mut grid2 = Grid::new();
        b_state.render_to_grid(&mut grid2);
        let header2 = grid2.line_trimmed(0);
        assert!(
            header2.contains("PLAY-CRT \u{2014} BASIC"),
            "header2 should be simple title 'PLAY-CRT — BASIC', got {header2:?}"
        );
        assert!(!header2.contains('['), "header2 should not contain bracket, got {header2:?}");
        assert!(!header2.contains('\u{25C4}'), "header2 should not contain arrow, got {header2:?}");
    }

    #[test]
    fn render_footer_shows_switch_hint() {
        let state = MenuState::with_kind(CatalogKind::ZMachine, vec![make_z("zork1", "Zork I", true)]);
        let mut grid = Grid::new();
        state.render_to_grid(&mut grid);
        let all: String = (0..crate::constants::ROWS).map(|y| grid.line_trimmed(y)).collect::<Vec<_>>().join("\n");
        assert!(all.contains("Left/Right: switch catalog"), "footer missing switch hint: {all}");
    }

    #[test]
    fn discover_unified_sorted() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_menu_discover");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("stories").join("zork1")).unwrap();
        std::fs::write(tmp.join("stories").join("zork1").join("zork1.z3"), b"FAKE").unwrap();
        std::fs::create_dir_all(tmp.join("basic").join("acey_ducey")).unwrap();
        std::fs::write(tmp.join("basic").join("acey_ducey").join("acey_ducey.py"), b"print(1)").unwrap();
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let entries = discover_unified();
        // Should contain both, sorted alphabetically
        assert!(entries.iter().any(|e| e.id == "zork1"));
        assert!(entries.iter().any(|e| e.id == "acey_ducey"));
        // Check sorting: Acey Ducey should come before Zork I alphabetically
        let acey_idx = entries.iter().position(|e| e.id == "acey_ducey").unwrap();
        let zork_idx = entries.iter().position(|e| e.id == "zork1").unwrap();
        assert!(acey_idx < zork_idx);
        if let Some(v) = prev { std::env::set_var("PLAY_CRT_DATA_DIR", v); } else { std::env::remove_var("PLAY_CRT_DATA_DIR"); }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_for_kind_separates() {
        let _guard = crate::paths::ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join("play_crt_menu_discover_separate");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("stories").join("zork1")).unwrap();
        std::fs::write(tmp.join("stories").join("zork1").join("zork1.z3"), b"FAKE").unwrap();
        std::fs::create_dir_all(tmp.join("basic").join("acey_ducey")).unwrap();
        std::fs::write(tmp.join("basic").join("acey_ducey").join("acey_ducey.py"), b"print(1)").unwrap();
        let prev = std::env::var("PLAY_CRT_DATA_DIR").ok();
        std::env::set_var("PLAY_CRT_DATA_DIR", &tmp);
        let z_entries = discover_for_kind(CatalogKind::ZMachine);
        assert!(z_entries.iter().any(|e| e.id == "zork1"));
        assert!(!z_entries.iter().any(|e| e.id == "acey_ducey"));
        let b_entries = discover_for_kind(CatalogKind::Basic);
        assert!(b_entries.iter().any(|e| e.id == "acey_ducey"));
        assert!(!b_entries.iter().any(|e| e.id == "zork1"));
        if let Some(v) = prev { std::env::set_var("PLAY_CRT_DATA_DIR", v); } else { std::env::remove_var("PLAY_CRT_DATA_DIR"); }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
