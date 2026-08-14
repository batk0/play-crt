use std::path::PathBuf;

use crate::grid::Grid;
use crate::saves;

/// Slot selection submenu — 3 slots per game, pure text.
pub struct SlotMenuState {
    pub(crate) game_id: String,
    pub(crate) title: String,
    pub(crate) game_path: PathBuf,
    pub(crate) entries: Vec<Option<saves::SaveMeta>>,
    pub(crate) selected: usize,
}

impl SlotMenuState {
    pub fn new(game_id: String, title: String, game_path: PathBuf) -> Self {
        let entries = saves::list_slots(&game_id);
        Self {
            game_id,
            title,
            game_path,
            entries,
            selected: 0,
        }
    }

    pub(crate) fn refresh(&mut self) {
        self.entries = saves::list_slots(&self.game_id);
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    pub(crate) fn move_up(&mut self) {
        if self.selected == 0 {
            self.selected = self.entries.len().saturating_sub(1);
        } else {
            self.selected -= 1;
        }
    }

    pub(crate) fn move_down(&mut self) {
        self.selected = (self.selected + 1) % self.entries.len().max(1);
    }

    pub(crate) fn selected_slot(&self) -> u8 {
        u8::try_from(self.selected + 1).expect("slot index fits in u8")
    }

    pub(crate) fn render_to_grid(&self, grid: &mut Grid) {
        grid.clear();
        let mut header = format!(" SELECT SAVE SLOT FOR {}", self.title.to_uppercase());
        if header.chars().count() > 78 {
            header = header.chars().take(78).collect();
        }
        grid.put_str(&format!("{header}\n"));
        grid.put_str(" ────────────────────────────────────────────────────────────────────────────────\n");
        for (idx, entry) in self.entries.iter().enumerate() {
            let is_sel = idx == self.selected;
            let marker = if is_sel { ">" } else { " " };
            let slot_no = idx + 1;
            let line = if let Some(meta) = entry {
                let mut status = meta.status.clone();
                // Trim status to fit: " 1. Slot 1 [West of House - 10/0 2026-05-13]" ~ 80 (char-boundary safe)
                if status.chars().count() > 36 {
                    status = status.chars().take(36).collect();
                }
                let mut ts = meta.timestamp.clone();
                if ts.chars().count() > 16 {
                    ts = ts.chars().take(16).collect();
                }
                format!(" {marker} {slot_no}. Slot {slot_no} [{status} {ts}]\n")
            } else {
                format!(" {marker} {slot_no}. Slot {slot_no} [Empty]\n")
            };
            grid.put_str(&line);
        }
        grid.put_str(" ────────────────────────────────────────────────────────────────────────────────\n");
        grid.put_str(" Enter: select  Up/Down: select  1-3: jump  B: back  Q: quit\n");
    }
}
