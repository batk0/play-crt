#![allow(clippy::too_many_lines)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::Instant;

use sdl2::keyboard::Keycode;

use crate::backend::ZMachineSession;
use crate::catalog;
use crate::controls::ControlState;
use crate::grid::Grid;
use crate::menu::MenuState;
use crate::saves;
use crate::slot_menu::SlotMenuState;

pub struct AppState {
    pub(crate) grid: Grid,
    pub(crate) input_buf: String,
    pub(crate) input_history: VecDeque<String>,
    pub(crate) history_idx: Option<usize>,
    pub(crate) session: Option<ZMachineSession>,
    pub(crate) vm_error: Option<String>,
    pub(crate) story_path: Option<PathBuf>,
    pub(crate) blink_on: bool,
    pub(crate) last_blink: Instant,
    pub(crate) start_time: Instant,
    pub(crate) control_state: ControlState,
    pub(crate) mouse_pos: Option<(i32, i32)>,
    pub(crate) menu: Option<MenuState>,
    pub(crate) slot_menu: Option<SlotMenuState>,
}

impl AppState {
    pub fn new(
        story_path: Option<PathBuf>,
        vm_error: Option<String>,
        session: Option<ZMachineSession>,
    ) -> Self {
        Self {
            grid: Grid::new(),
            input_buf: String::new(),
            input_history: VecDeque::new(),
            history_idx: None,
            session,
            vm_error,
            story_path,
            blink_on: true,
            last_blink: Instant::now(),
            start_time: Instant::now(),
            control_state: ControlState::default(),
            mouse_pos: None,
            menu: None,
            slot_menu: None,
        }
    }

    pub fn new_with_menu(menu: MenuState) -> Self {
        let mut s = Self {
            grid: Grid::new(),
            input_buf: String::new(),
            input_history: VecDeque::new(),
            history_idx: None,
            session: None,
            vm_error: None,
            story_path: None,
            blink_on: true,
            last_blink: Instant::now(),
            start_time: Instant::now(),
            control_state: ControlState::default(),
            mouse_pos: None,
            menu: Some(menu),
            slot_menu: None,
        };
        if let Some(m) = &s.menu {
            m.render_to_grid(&mut s.grid);
        }
        s
    }

    pub(crate) fn is_menu_active(&self) -> bool {
        self.menu.is_some() || self.slot_menu.is_some()
    }

    #[allow(dead_code)]
    pub(crate) fn is_slot_menu_active(&self) -> bool {
        self.slot_menu.is_some()
    }

    pub(crate) fn enter_slot_menu(&mut self) -> bool {
        let Some(menu) = &self.menu else {
            return false;
        };
        let Some(entry) = menu.selected_entry().cloned() else {
            return false;
        };
        if !entry.is_downloaded {
            return false;
        }
        let Some(path) = entry.local_path.clone() else {
            return false;
        };
        let game_id = entry.id.clone();
        let title = entry.title.clone();
        let mut slot_state = SlotMenuState::new(game_id, title, path);
        slot_state.refresh();
        slot_state.render_to_grid(&mut self.grid);
        self.slot_menu = Some(slot_state);
        true
    }

    pub(crate) fn launch_with_slot(&mut self, slot: u8) -> bool {
        let Some(slot_state) = &self.slot_menu else {
            return false;
        };
        let game_id = slot_state.game_id.clone();
        let path = slot_state.game_path.clone();
        match ZMachineSession::new_with_slot(path.clone(), game_id.clone(), slot) {
            Ok(sess) => {
                self.session = Some(sess);
                self.story_path = Some(path);
                self.vm_error = None;
                self.menu = None;
                self.slot_menu = None;
                self.grid.clear();
                true
            }
            Err(e) => {
                if let Some(m) = self.menu.as_mut() {
                    m.status_msg = Some(format!("Failed to start: {e}"));
                }
                // Return to game menu on failure
                self.slot_menu = None;
                if let Some(m) = &self.menu {
                    m.render_to_grid(&mut self.grid);
                }
                false
            }
        }
    }

    /// Attempt to launch the currently selected menu entry (if downloaded).
    /// Returns true if a session was started.
    #[allow(dead_code)]
    pub(crate) fn launch_selected(&mut self) -> bool {
        let Some(menu) = &self.menu else {
            return false;
        };
        let Some(entry) = menu.selected_entry().cloned() else {
            return false;
        };
        if !entry.is_downloaded {
            return false;
        }
        let Some(path) = entry.local_path.clone() else {
            return false;
        };
        match ZMachineSession::new(path.clone()) {
            Ok(sess) => {
                self.session = Some(sess);
                self.story_path = Some(path.clone());
                self.vm_error = None;
                self.menu = None;
                self.grid.clear();
                true
            }
            Err(e) => {
                // Update status and re-render menu
                let (entries, selected, downloading, status_msg) = {
                    if let Some(m) = self.menu.as_mut() {
                        m.status_msg = Some(format!("Failed to start: {e}"));
                        (
                            m.entries.clone(),
                            m.selected,
                            m.downloading.clone(),
                            m.status_msg.clone(),
                        )
                    } else {
                        return false;
                    }
                };
                let tmp = MenuState {
                    entries,
                    selected,
                    downloading,
                    status_msg,
                    dl_rx: None,
                };
                self.grid.clear();
                tmp.render_to_grid(&mut self.grid);
                false
            }
        }
    }

    pub(crate) fn handle_menu_key(&mut self, keycode: Keycode) -> bool {
        // Return true if app should quit
        // If slot menu is active, delegate to slot handling
        if self.slot_menu.is_some() {
            return self.handle_slot_menu_key(keycode);
        }
        match keycode {
            Keycode::Up => {
                if let Some(menu) = self.menu.as_mut() {
                    menu.move_up();
                }
                if let Some(m) = &self.menu {
                    m.render_to_grid(&mut self.grid);
                }
            }
            Keycode::Down => {
                if let Some(menu) = self.menu.as_mut() {
                    menu.move_down();
                }
                if let Some(m) = &self.menu {
                    m.render_to_grid(&mut self.grid);
                }
            }
            Keycode::Return | Keycode::KpEnter => {
                let is_downloading = self
                    .menu
                    .as_ref()
                    .and_then(|m| m.downloading.clone())
                    .is_some();
                if is_downloading {
                    return false;
                }
                let is_dl = self
                    .menu
                    .as_ref()
                    .and_then(|m| m.selected_entry())
                    .is_some_and(|e| !e.is_downloaded);
                if is_dl {
                    if let Some(menu) = self.menu.as_mut() {
                        menu.start_download();
                    }
                    if let Some(m) = &self.menu {
                        m.render_to_grid(&mut self.grid);
                    }
                } else {
                    let should_launch = self
                        .menu
                        .as_ref()
                        .and_then(|m| m.selected_entry())
                        .is_some_and(|e| e.is_downloaded);
                    if should_launch {
                        // Transition to slot selection instead of launching directly
                        self.enter_slot_menu();
                    }
                }
            }
            Keycode::R => {
                if let Some(menu) = self.menu.as_mut() {
                    menu.refresh();
                }
                if let Some(m) = &self.menu {
                    m.render_to_grid(&mut self.grid);
                }
            }
            Keycode::Q => return true,
            _ => {}
        }
        false
    }

    pub(crate) fn handle_slot_menu_key(&mut self, keycode: Keycode) -> bool {
        match keycode {
            Keycode::Up => {
                if let Some(sm) = self.slot_menu.as_mut() {
                    sm.move_up();
                }
                if let Some(sm) = &self.slot_menu {
                    sm.render_to_grid(&mut self.grid);
                }
            }
            Keycode::Down => {
                if let Some(sm) = self.slot_menu.as_mut() {
                    sm.move_down();
                }
                if let Some(sm) = &self.slot_menu {
                    sm.render_to_grid(&mut self.grid);
                }
            }
            Keycode::Return | Keycode::KpEnter => {
                let slot = self.slot_menu.as_ref().map_or(1, SlotMenuState::selected_slot);
                self.launch_with_slot(slot);
            }
            Keycode::B => {
                // Back to game list
                self.slot_menu = None;
                if let Some(m) = &self.menu {
                    m.render_to_grid(&mut self.grid);
                }
            }
            Keycode::Q => return true,
            Keycode::Escape => {
                self.slot_menu = None;
                if let Some(m) = &self.menu {
                    m.render_to_grid(&mut self.grid);
                }
            }
            _ => {}
        }
        false
    }

    pub(crate) fn handle_menu_text(&mut self, text: &str) {
        // If slot menu is active, digits 1-3 select slot, b/q for nav
        if self.slot_menu.is_some() {
            let t = text.trim().to_ascii_lowercase();
            if t == "b" {
                self.slot_menu = None;
                if let Some(m) = &self.menu {
                    m.render_to_grid(&mut self.grid);
                }
                return;
            }
            if t == "q" {
                return;
            }
            if let Some(ch) = t.chars().next() {
                if ch.is_ascii_digit() && ch != '0' {
                    let slot_idx = (ch as usize) - ('1' as usize);
                    if slot_idx < saves::NUM_SLOTS as usize {
                        if let Some(sm) = self.slot_menu.as_mut() {
                            sm.selected = slot_idx;
                            sm.render_to_grid(&mut self.grid);
                        }
                        // Immediately launch that slot (number selects + starts)
                        let slot = u8::try_from(slot_idx + 1).expect("slot index fits in u8");
                        self.launch_with_slot(slot);
                    }
                }
            }
            return;
        }
        // Digits 1-9 jump to entry, q/r shortcuts as text as well
        let t = text.trim().to_ascii_lowercase();
        if t == "q" {
            // Handled via key but also text input
            return;
        }
        if t == "r" {
            if let Some(menu) = self.menu.as_mut() {
                menu.refresh();
            }
            if let Some(m) = &self.menu {
                m.render_to_grid(&mut self.grid);
            }
            return;
        }
        if t == "b" && self.slot_menu.is_some() {
            self.slot_menu = None;
            if let Some(m) = &self.menu {
                m.render_to_grid(&mut self.grid);
            }
            return;
        }
        if let Some(ch) = t.chars().next() {
            if ch.is_ascii_digit() && ch != '0' {
                let idx = (ch as usize) - ('1' as usize);
                // Capture decision without holding borrow across launch
                let should = {
                    let Some(menu) = self.menu.as_mut() else {
                        return;
                    };
                    if idx >= menu.entries.len() {
                        return;
                    }
                    menu.selected = idx;
                    let is_dl = !menu.entries[idx].is_downloaded;
                    if is_dl {
                        menu.start_download();
                        None // handled: download started
                    } else {
                        Some(false) // should show slot menu
                    }
                };
                if should.is_none() {
                    if let Some(m) = &self.menu {
                        m.render_to_grid(&mut self.grid);
                    }
                    return;
                }
                self.enter_slot_menu();
            }
        }
    }

    pub(crate) fn poll_menu_download(&mut self) {
        // Need to poll without holding borrow across launch
        let poll_result = {
            let Some(menu) = self.menu.as_mut() else {
                return;
            };
            menu.poll_download()
        };
        let Some(res) = poll_result else {
            // Still downloading — re-render to keep spinner? Keep current grid.
            return;
        };
        match res {
            Ok(path) => {
                // Refresh entries to mark downloaded, then show slot menu
                if let Some(menu) = self.menu.as_mut() {
                    menu.refresh();
                    // Find index of newly downloaded entry by path
                    if let Some(idx) = menu
                        .entries
                        .iter()
                        .position(|e| e.local_path.as_ref() == Some(&path))
                    {
                        menu.selected = idx;
                    }
                    menu.status_msg = Some("Downloaded — select a save slot".to_string());
                }
                // Transition to slot selection instead of auto-launching
                self.enter_slot_menu();
            }
            Err(e) => {
                if let Some(menu) = self.menu.as_mut() {
                    menu.status_msg = Some(format!("Download failed: {e}"));
                    let m = self.menu.as_ref().unwrap();
                    m.render_to_grid(&mut self.grid);
                }
            }
        }
    }

    pub(crate) fn return_to_menu(&mut self, reason: &str) {
        // Drop any active Z-machine session and return to the 80×24 story picker.
        // Always return to the Games menu (not the slot menu) so the player can
        // pick a different game and then a different slot.
        let _ = self.session.take();
        self.input_buf.clear();
        self.history_idx = None;
        self.vm_error = None;
        self.story_path = None;
        self.slot_menu = None;
        let mut menu = MenuState::new(catalog::discover());
        menu.status_msg = Some(reason.to_string());
        menu.render_to_grid(&mut self.grid);
        self.menu = Some(menu);
    }

    pub(crate) fn seed_banner(&mut self, _font_path: &Path, _pt: u16) {
        self.grid
            .put_str(" ZORK CRT  •  SDL2 phosphor  •  80×24  •  VT323\n");
        self.grid
            .put_str(" Z-machine: pure Rust (encrusted, MIT) • 80×24 • no external frotz\n");
        self.grid.put_str(
            " ────────────────────────────────────────────────────────────────────────────────\n",
        );
    }

    pub(crate) fn handle_backspace(&mut self) {
        if self.input_buf.is_empty() {
            return;
        }
        self.input_buf.pop();
        if self.grid.cursor_x > 0 {
            self.grid.cursor_x -= 1;
            self.grid.cells[self.grid.cursor_y][self.grid.cursor_x] = ' ';
        } else if self.grid.cursor_y > 0 {
            self.grid.cursor_y -= 1;
            self.grid.cursor_x = crate::constants::COLS - 1;
            self.grid.cells[self.grid.cursor_y][self.grid.cursor_x] = ' ';
        }
        self.grid.dirty = true;
    }

    pub(crate) fn handle_text_input(&mut self, text: &str) {
        if text == "\r" || text == "\n" {
            return;
        }
        let filtered: String = text.chars().filter(|c| !c.is_control()).collect();
        if filtered.is_empty() {
            return;
        }
        if self.input_buf.len() + filtered.len() > 200 {
            return;
        }
        for ch in filtered.chars() {
            self.input_buf.push(ch);
            if self.grid.cursor_y < crate::constants::ROWS
                && self.grid.cursor_x < crate::constants::COLS
            {
                self.grid.cells[self.grid.cursor_y][self.grid.cursor_x] = ch;
                self.grid.cursor_x += 1;
                if self.grid.cursor_x >= crate::constants::COLS {
                    self.grid.cursor_x = 0;
                    self.grid.cursor_y += 1;
                    if self.grid.cursor_y >= crate::constants::ROWS {
                        self.grid.cursor_y = crate::constants::ROWS - 1;
                        for y in 1..crate::constants::ROWS {
                            self.grid.cells[y - 1] = self.grid.cells[y];
                        }
                        self.grid.cells[crate::constants::ROWS - 1] = [' '; crate::constants::COLS];
                    }
                }
            }
        }
        self.grid.dirty = true;
    }

    pub(crate) fn history_prev(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        let idx = match self.history_idx {
            None => self.input_history.len().saturating_sub(1),
            Some(0) => 0,
            Some(n) => n - 1,
        };
        self.history_idx = Some(idx);
        self.replace_input_with_history(idx);
    }

    pub(crate) fn history_next(&mut self) {
        let Some(idx) = self.history_idx else { return };
        for _ in 0..self.input_buf.len() {
            if self.grid.cursor_x > 0 {
                self.grid.cursor_x -= 1;
                self.grid.cells[self.grid.cursor_y][self.grid.cursor_x] = ' ';
            }
        }
        if idx + 1 < self.input_history.len() {
            let nidx = idx + 1;
            self.history_idx = Some(nidx);
            self.input_buf.clone_from(&self.input_history[nidx]);
            self.echo_input_buf();
        } else {
            self.history_idx = None;
            self.input_buf.clear();
        }
        self.grid.dirty = true;
    }

    fn replace_input_with_history(&mut self, idx: usize) {
        for _ in 0..self.input_buf.len() {
            if self.grid.cursor_x > 0 {
                self.grid.cursor_x -= 1;
                self.grid.cells[self.grid.cursor_y][self.grid.cursor_x] = ' ';
            }
        }
        self.input_buf.clone_from(&self.input_history[idx]);
        self.echo_input_buf();
        self.grid.dirty = true;
    }

    fn echo_input_buf(&mut self) {
        let cloned = self.input_buf.clone();
        for ch in cloned.chars() {
            if self.grid.cursor_x < crate::constants::COLS
                && self.grid.cursor_y < crate::constants::ROWS
            {
                self.grid.cells[self.grid.cursor_y][self.grid.cursor_x] = ch;
                self.grid.cursor_x += 1;
                if self.grid.cursor_x >= crate::constants::COLS {
                    self.grid.newline();
                }
            }
        }
    }

    pub(crate) fn submit_input(&mut self) {
        if self.is_menu_active() {
            return;
        }
        if self.session.is_some() {
            let line = self.input_buf.clone();
            self.grid.newline();
            if !line.is_empty() {
                if self.input_history.len() >= 100 {
                    self.input_history.pop_front();
                }
                self.input_history.push_back(line.clone());
            }
            self.history_idx = None;
            if let Some(sess) = self.session.as_mut() {
                if let Err(e) = sess.send_input(&line) {
                    self.grid
                        .put_str(&format!("\n !! Z-machine input failed: {e}\n"));
                }
            }
            self.input_buf.clear();
        } else {
            self.grid.newline();
            self.input_buf.clear();
        }
    }

    pub(crate) fn poll_zmachine(&mut self) {
        let Some(sess) = self.session.as_mut() else {
            return;
        };
        let mut disconnected = false;
        loop {
            match sess.rx.try_recv() {
                Ok(chunk) => {
                    if let Some(filtered) = Self::sanitize_chunk(&chunk) {
                        // Suppress the backend's "Game ended" banner — we show
                        // our own status_msg in the menu instead.
                        if filtered.contains("[Game ended") {
                            continue;
                        }
                        self.grid.put_str(&filtered);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            self.return_to_menu("Game ended — Returned to menu");
        }
    }

    pub(crate) fn check_session_exit(&mut self) -> bool {
        if let Some(sess) = self.session.as_mut() {
            match sess.rx.try_recv() {
                Ok(c) => {
                    if let Some(filtered) = Self::sanitize_chunk(&c) {
                        if filtered.contains("[Game ended") {
                            // Don't render backend banner; poll_session will show menu status
                            return false;
                        }
                        self.grid.put_str(&filtered);
                    }
                    return false;
                }
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => return true,
            }
        }
        false
    }

    /// Defensive filter: raw Quetzal / savestate JSON must never reach the
    /// phosphor grid. The backend `GuiUi::message` already suppresses
    /// `savestate`/`save`/`restore` control messages, but as a second layer
    /// we strip any chunk that still contains them (e.g. `>[savestate]
    /// ["West of House - 0/0", "Rk9S..."]`). Keeps text before the marker
    /// (like a `>` prompt) and drops the control payload.
    pub(crate) fn sanitize_chunk(chunk: &str) -> Option<String> {
        // Fast path: no control marker
        if !chunk.contains("[savestate]")
            && !chunk.contains("[save]")
            && !chunk.contains("[restore]")
        {
            return Some(chunk.to_string());
        }
        // Contains a control marker — strip it and everything after on that line.
        // Keep prefix before the marker (e.g. the `>` prompt).
        let mut out = String::new();
        for line in chunk.split_inclusive('\n') {
            if line.contains("[savestate]") || line.contains("[save]") || line.contains("[restore]") {
                if let Some(idx) = line.find("[savestate]") {
                    out.push_str(&line[..idx]);
                } else if let Some(idx) = line.find("[save]") {
                    out.push_str(&line[..idx]);
                } else if let Some(idx) = line.find("[restore]") {
                    out.push_str(&line[..idx]);
                }
                // drop the rest of the line (the JSON/base64)
                // ensure we still terminate the line if original had newline
                if line.ends_with('\n') {
                    out.push('\n');
                }
                // Log for debugging, but don't render
                if std::env::var("DEBUG").is_ok() {
                    eprintln!("[sanitize] dropped control line: {:?}", line.chars().take(80).collect::<String>());
                }
            } else {
                out.push_str(line);
            }
        }
        if out.trim().is_empty() {
            None
        } else {
            Some(out)
        }
    }
}
