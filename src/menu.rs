use std::path::PathBuf;

use crate::catalog;
use crate::download;
use crate::grid::Grid;

/// Pure-text game picker rendered into the 80×24 CRT grid.
pub struct MenuState {
    pub(crate) entries: Vec<catalog::GameEntry>,
    pub(crate) selected: usize,
    pub(crate) downloading: Option<String>,
    pub(crate) status_msg: Option<String>,
    pub(crate) dl_rx: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
}

impl MenuState {
    pub fn new(entries: Vec<catalog::GameEntry>) -> Self {
        Self {
            entries,
            selected: 0,
            downloading: None,
            status_msg: None,
            dl_rx: None,
        }
    }

    pub(crate) fn refresh(&mut self) {
        self.entries = catalog::discover();
        if self.selected >= self.entries.len() && !self.entries.is_empty() {
            self.selected = self.entries.len() - 1;
        }
        self.status_msg = Some(format!("Refreshed — {} games.", self.entries.len()));
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

    pub(crate) fn selected_entry(&self) -> Option<&catalog::GameEntry> {
        self.entries.get(self.selected)
    }

    pub(crate) fn start_download(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if entry.is_downloaded || self.downloading.is_some() {
            return;
        }
        let id = entry.id.clone();
        self.downloading = Some(id.clone());
        self.status_msg = Some(format!("Downloading {} ...", entry.title));
        let (tx, rx) = std::sync::mpsc::channel();
        self.dl_rx = Some(rx);
        std::thread::spawn(move || {
            let res = download::download(&entry, |_| {});
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
        // Title bar
        grid.put_str(" PLAY-CRT  --  SELECT A STORY\n");
        grid.put_str(" ────────────────────────────────────────────────────────────────────────────────\n");
        if self.entries.is_empty() {
            grid.put_str("\n No games found.\n");
            grid.put_str(" Add .z3/.z5/.z8 files to the stories folder or check the manifest.\n");
            grid.put_str("\n [R] Refresh   [Q] Quit\n");
            if let Some(msg) = &self.status_msg {
                grid.put_str(&format!("\n {msg}\n"));
            }
            return;
        }
        // List entries (max ~18 to fit 24 rows with header/footer)
        let max_visible = 16usize;
        let offset = if self.entries.len() <= max_visible || self.selected < max_visible / 2 {
            0
        } else if self.selected >= self.entries.len() - max_visible / 2 {
            self.entries.len() - max_visible
        } else {
            self.selected - max_visible / 2
        };
        for (idx, entry) in self.entries.iter().enumerate().skip(offset).take(max_visible) {
            let is_sel = idx == self.selected;
            let marker = if is_sel { ">" } else { " " };
            let state = if Some(entry.id.as_str()) == self.downloading.as_deref() {
                "[Downloading...]"
            } else if entry.is_downloaded {
                "[Ready]"
            } else {
                "[Download]"
            };
            // Truncate title to fit 80 cols: " > 1. Title [State]" → ~76 for title
            let num = idx + 1;
            let mut title = entry.title.clone();
            // keep within 80 - (marker+num+state) ~ 6+10
            if title.len() > 58 {
                title.truncate(58);
            }
            let line = format!(" {marker} {num}. {title} {state}\n");
            // Highlight selected by inverse-like prefix already; keep plain for CRT grid
            grid.put_str(&line);
        }
        if self.entries.len() > max_visible {
            grid.put_str(&format!(
                " ... {} more (use Up/Down)\n",
                self.entries.len() - max_visible
            ));
        }
        grid.put_str(" ────────────────────────────────────────────────────────────────────────────────\n");
        if let Some(dl) = &self.downloading {
            grid.put_str(&format!(" Downloading {dl} ... please wait\n"));
        } else if let Some(msg) = &self.status_msg {
            // Trim to one line
            let mut m = msg.clone();
            if m.len() > 78 {
                m.truncate(78);
            }
            grid.put_str(&format!(" {m}\n"));
        }
        grid.put_str(" Enter: play/download  Up/Down: select  1-9: jump  R: refresh  Q: quit\n");
    }
}
