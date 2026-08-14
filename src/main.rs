mod backend;
mod catalog;
mod cli;
mod constants;
mod controls;
mod crt_gl;
mod crt_pi;
mod download;
mod font;
mod grid;
mod paths;
mod render;
mod zmachine;

use std::collections::VecDeque;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::{Duration, Instant};

use sdl2::event::Event;
use sdl2::keyboard::Keycode;

use backend::{find_story, ZMachineSession};
use constants::{
    f32_to_u8_clamped, u32_to_i32, usize_to_i32, BEZEL, INNER_PAD, WINDOW_H, WINDOW_W,
};
use controls::ControlState;
use grid::Grid;
use render::{
    compute_grid_metrics, draw_bezel, draw_bottom_control_labels, draw_bottom_controls,
    draw_glass, draw_grid_text_with_controls, draw_power_led,
    draw_scanlines_and_vignette_with_state, GridMetrics,
};

#[allow(dead_code)]
fn resolve_story_with_picker(cli_arg: Option<&String>) -> Option<PathBuf> {
    let mut story_path = find_story(cli_arg.cloned());
    if story_path.is_some() {
        return story_path;
    }
    if cli_arg.is_some() {
        return None;
    }
    eprintln!("No story file found via CLI or default search.");
    let should_pick = env::var("DISPLAY").is_ok()
        || env::var("WAYLAND_DISPLAY").is_ok()
        || cfg!(target_os = "macos");
    let in_ci = env::var("CI").is_ok() || env::var("GITHUB_ACTIONS").is_ok();
    if should_pick && !in_ci {
        let picked = rfd::FileDialog::new()
            .add_filter("Z-machine", &["z3", "z5", "z8", "zip"])
            .add_filter("All files", &["*"])
            .set_title("Choose a Z-machine story file (.z3/.z5/.z8/.zip)")
            .pick_file();
        if let Some(p) = picked {
            println!("picked story: {}", p.display());
            story_path = Some(p);
        } else {
            eprintln!("Will show in-GUI error screen; use --story or press F1 for picker.");
        }
    } else {
        eprintln!("Will show in-GUI error screen; use --story <path> (e.g. assets/stories/zork1.z3) or press F1 for picker.");
    }
    story_path
}

// ── Text-menu state (pure 80×24, no rfd/modal overlay) ────────────────

struct MenuState {
    entries: Vec<catalog::GameEntry>,
    selected: usize,
    downloading: Option<String>,
    status_msg: Option<String>,
    dl_rx: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
}

impl MenuState {
    fn new(entries: Vec<catalog::GameEntry>) -> Self {
        Self {
            entries,
            selected: 0,
            downloading: None,
            status_msg: None,
            dl_rx: None,
        }
    }

    fn refresh(&mut self) {
        self.entries = catalog::discover();
        if self.selected >= self.entries.len() && !self.entries.is_empty() {
            self.selected = self.entries.len() - 1;
        }
        self.status_msg = Some(format!("Refreshed — {} games.", self.entries.len()));
    }

    fn move_up(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.entries.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.entries.len();
    }

    fn selected_entry(&self) -> Option<&catalog::GameEntry> {
        self.entries.get(self.selected)
    }

    fn start_download(&mut self) {
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

    /// Poll download channel; returns Some(Ok(path)) or Some(Err(msg)).
    fn poll_download(&mut self) -> Option<Result<PathBuf, String>> {
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
    fn render_to_grid(&self, grid: &mut Grid) {
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

struct AppState {
    grid: Grid,
    input_buf: String,
    input_history: VecDeque<String>,
    history_idx: Option<usize>,
    session: Option<ZMachineSession>,
    vm_error: Option<String>,
    story_path: Option<PathBuf>,
    blink_on: bool,
    last_blink: Instant,
    start_time: Instant,
    control_state: ControlState,
    mouse_pos: Option<(i32, i32)>,
    menu: Option<MenuState>,
}

impl AppState {
    fn new(
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
        }
    }

    fn new_with_menu(menu: MenuState) -> Self {
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
        };
        if let Some(m) = &s.menu {
            m.render_to_grid(&mut s.grid);
        }
        s
    }

    fn is_menu_active(&self) -> bool {
        self.menu.is_some()
    }

    /// Attempt to launch the currently selected menu entry (if downloaded).
    /// Returns true if a session was started.
    fn launch_selected(&mut self) -> bool {
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

    fn handle_menu_key(&mut self, keycode: Keycode) -> bool {
        // Return true if app should quit
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
                        self.launch_selected();
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

    fn handle_menu_text(&mut self, text: &str) {
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
                        Some(false) // should launch
                    }
                };
                if should.is_none() {
                    if let Some(m) = &self.menu {
                        m.render_to_grid(&mut self.grid);
                    }
                    return;
                }
                self.launch_selected();
            }
        }
    }

    fn poll_menu_download(&mut self) {
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
                // Refresh entries to mark downloaded, then auto-launch
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
                    menu.status_msg = Some("Downloaded — starting...".to_string());
                    // Render one frame before launch
                    let m = self.menu.as_ref().unwrap();
                    m.render_to_grid(&mut self.grid);
                }
                // Auto-launch
                self.launch_selected();
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

    fn return_to_menu(&mut self, reason: &str) {
        // Drop any active Z-machine session and return to the 80×24 story picker.
        let _ = self.session.take();
        self.input_buf.clear();
        self.history_idx = None;
        self.vm_error = None;
        self.story_path = None;
        let mut menu = MenuState::new(catalog::discover());
        menu.status_msg = Some(reason.to_string());
        menu.render_to_grid(&mut self.grid);
        self.menu = Some(menu);
    }

    fn seed_banner(&mut self, _font_path: &Path, _pt: u16) {
        self.grid
            .put_str(" ZORK CRT  •  SDL2 phosphor  •  80×24  •  VT323\n");
        self.grid
            .put_str(" Z-machine: pure Rust (encrusted, MIT) • 80×24 • no external frotz\n");
        self.grid.put_str(
            " ────────────────────────────────────────────────────────────────────────────────\n",
        );
    }

    fn handle_backspace(&mut self) {
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

    fn handle_text_input(&mut self, text: &str) {
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

    fn history_prev(&mut self) {
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

    fn history_next(&mut self) {
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

    fn submit_input(
        &mut self,
        video: &sdl2::VideoSubsystem,
        canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
        event_pump: &mut sdl2::EventPump,
    ) {
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
        } else if self.story_path.is_none() {
            let picked = rfd::FileDialog::new()
                .add_filter("Z-machine", &["z3", "z5", "z8", "zip"])
                .pick_file();
            restore_focus(video, canvas, event_pump);
            if let Some(p) = picked {
                match ZMachineSession::new(p.clone()) {
                    Ok(s) => {
                        self.grid.clear();
                        self.session = Some(s);
                        self.story_path = Some(p);
                        self.vm_error = None;
                    }
                    Err(e) => self.grid.put_str(&format!("\n !! {e}\n")),
                }
            }
        } else {
            self.grid.newline();
            self.input_buf.clear();
        }
    }

    fn poll_zmachine(&mut self) {
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

    fn check_session_exit(&mut self) -> bool {
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
    fn sanitize_chunk(chunk: &str) -> Option<String> {
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

fn restore_focus(
    video: &sdl2::VideoSubsystem,
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    event_pump: &mut sdl2::EventPump,
) {
    // `rfd` native dialogs steal OS focus; SDL does not automatically reclaim it.
    // Re-raise the SDL window and re-enable TextInput so keyboard goes to the game,
    // not the terminal that launched the app.
    canvas.window_mut().raise();
    // Best-effort raw focus request (SDL_SetWindowInputFocus exists in sdl2-sys 0.38
    // but is not wrapped by the sdl2 crate). Failure is non-fatal.
    unsafe {
        sdl2::sys::SDL_SetWindowInputFocus(canvas.window().raw());
        sdl2::sys::SDL_StartTextInput();
    }
    video.text_input().start();
    // Pump once and discard stale events accumulated while the modal dialog was open
    event_pump.pump_events();
    for _ in event_pump.poll_iter() {}
    // Give the window manager a moment to deliver focus (notably needed on macOS)
    thread::sleep(Duration::from_millis(50));
    canvas.window_mut().raise();
    unsafe {
        sdl2::sys::SDL_SetWindowInputFocus(canvas.window().raw());
    }
    video.text_input().start();
}

#[allow(dead_code)]
fn resolve_story_with_window(
    cli_arg: Option<&String>,
    video: &sdl2::VideoSubsystem,
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    event_pump: &mut sdl2::EventPump,
) -> Option<PathBuf> {
    if let Some(p) = find_story(cli_arg.cloned()) {
        return Some(p);
    }
    if cli_arg.is_some() {
        return None;
    }
    eprintln!("No story file found via CLI or default search.");
    let should_pick = env::var("DISPLAY").is_ok()
        || env::var("WAYLAND_DISPLAY").is_ok()
        || cfg!(target_os = "macos");
    let in_ci = env::var("CI").is_ok() || env::var("GITHUB_ACTIONS").is_ok();
    if should_pick && !in_ci {
        let picked = rfd::FileDialog::new()
            .add_filter("Z-machine", &["z3", "z5", "z8", "zip"])
            .add_filter("All files", &["*"])
            .set_title("Choose a Z-machine story file (.z3/.z5/.z8/.zip)")
            .pick_file();
        // rfd steals OS focus; restore SDL window focus/text input even if cancelled.
        restore_focus(video, canvas, event_pump);
        // Ensure text input is re-enabled after the modal dialog.
        video.text_input().start();
        if let Some(p) = picked {
            println!("picked story: {}", p.display());
            return Some(p);
        }
        eprintln!("Will show in-GUI error screen; use --story or press F1 for picker.");
    } else {
        eprintln!(
            "Will show in-GUI error screen; use --story <path> (e.g. assets/stories/zork1.z3) or press F1 for picker."
        );
    }
    None
}

fn handle_f1_picker(
    state: &mut AppState,
    video: &sdl2::VideoSubsystem,
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    event_pump: &mut sdl2::EventPump,
) {
    let picked = rfd::FileDialog::new()
        .add_filter("Z-machine", &["z3", "z5", "z8", "zip"])
        .pick_file();
    // Always restore SDL focus/text-input, even when the dialog is cancelled.
    restore_focus(video, canvas, event_pump);
    let Some(p) = picked else {
        return;
    };
    // Drop previous session (closes channel, VM thread exits)
    let _ = state.session.take();
    match ZMachineSession::new(p.clone()) {
        Ok(s) => {
            state.grid.clear();
            state.session = Some(s);
            state.story_path = Some(p);
            state.vm_error = None;
            state.input_buf.clear();
        }
        Err(e) => {
            state.grid.put_str(&format!("\n !! spawn failed: {e}\n"));
            state.vm_error = Some(e);
        }
    }
}

fn init_sdl() -> Result<(sdl2::Sdl, sdl2::VideoSubsystem, sdl2::ttf::Sdl2TtfContext), String> {
    let sdl = sdl2::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let ttf = sdl2::ttf::init().map_err(|e| e.to_string())?;
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");
    Ok((sdl, video, ttf))
}

fn create_window(
    video: &sdl2::VideoSubsystem,
) -> Result<sdl2::render::Canvas<sdl2::video::Window>, String> {
    // Request GL 3.3 core for the optional crt-pi shader path (glow).
    // This is set before window creation and is harmless for the SDL Canvas path.
    {
        let gl_attr = video.gl_attr();
        gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
        gl_attr.set_context_version(3, 3);
        gl_attr.set_depth_size(0);
        gl_attr.set_stencil_size(0);
    }
    let window = video
        .window(
            "ZORK I — CRT  •  80×24  •  VT323 phosphor (crt-pi)",
            WINDOW_W,
            WINDOW_H,
        )
        .position_centered()
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;
    let canvas = window
        .into_canvas()
        .accelerated()
        .present_vsync()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(canvas)
}

fn setup_font_and_metrics(
    ttf: &sdl2::ttf::Sdl2TtfContext,
) -> Result<(sdl2::ttf::Font<'_, 'static>, PathBuf, u16, GridMetrics), String> {
    let grid_w = u32::try_from(constants::window_w_i32() - BEZEL * 2 - INNER_PAD * 2)
        .expect("grid_w positive");
    let grid_h = u32::try_from(constants::window_h_i32() - BEZEL * 2 - INNER_PAD * 2)
        .expect("grid_h positive");

    let (mut font, font_path, pt) = font::choose_font(ttf, grid_w, grid_h)?;
    font.set_style(sdl2::ttf::FontStyle::NORMAL);
    font.set_hinting(sdl2::ttf::Hinting::Light);

    let (cell_w, cell_h) = {
        let sample = "M".repeat(constants::COLS);
        let (w, _) = font.size_of(&sample).unwrap_or((grid_w, 20));
        let cw = w / constants::cols_u32();
        let lh_i32 = font.recommended_line_spacing();
        let lh = u32::try_from(lh_i32).unwrap_or(12);
        (cw.max(1), lh.max(12))
    };
    if std::env::var("DEBUG").is_ok() {
        eprintln!(
            "font: {} @ {pt}pt  cell≈{}×{}  grid={}×{}  inner={}×{}",
            font_path.display(),
            cell_w,
            cell_h,
            grid_w,
            grid_h,
            WINDOW_W,
            WINDOW_H
        );
    }
    let metrics = compute_grid_metrics(INNER_PAD, cell_w, cell_h);
    Ok((font, font_path, pt, metrics))
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<(), String> {
    let cli = cli::parse_args()?;
    if cli.show_help {
        cli::print_help();
        return Ok(());
    }
    if cli.show_version {
        println!("play-crt 0.1.0 (SDL2 CRT, pure Rust Z-machine — encrusted/MIT)");
        return Ok(());
    }

    let story_arg = cli.story_arg.clone();
    if let Some((curv_x, curv_y)) = cli.curvature {
        let p = crt_pi::CrtPiParams {
            curvature_x: curv_x,
            curvature_y: curv_y,
            ..crt_pi::CrtPiParams::default()
        };
        crt_pi::set_curvature_override(p);
        if std::env::var("DEBUG").is_ok() {
            eprintln!("curvature override: CURVATURE_X={curv_x:.2} CURVATURE_Y={curv_y:.2} (default 0.20,0.20)");
        }
    } else if std::env::var("DEBUG").is_ok() {
        let d = crt_pi::CrtPiParams::default();
        eprintln!(
            "curvature default: CURVATURE_X={:.2} CURVATURE_Y={:.2} (tune via --curvature 0.20 or 0.15,0.20)",
            d.curvature_x, d.curvature_y
        );
    }
    if story_arg.is_some() && find_story(story_arg.clone()).is_none() {
        return Err(format!(
            "Story file not found: {:?}. Pass --story <path> with existing file or use --story with a path under {}.",
            story_arg.expect("story_arg some"),
            paths::stories_dir().display()
        ));
    }

    // Ensure data layout exists early (creates stories/downloads/saves dirs)
    let _ = paths::ensure_layout();

    let (sdl, video, ttf) = init_sdl()?;
    let mut canvas = create_window(&video)?;
    // Optional crt-pi GL path: compile the shader at startup and keep it alive.
    let _crt_gl: Option<crt_gl::CrtGl> = match crt_gl::CrtGl::try_new(&video, canvas.window()) {
        Ok(g) => {
            if std::env::var("DEBUG").is_ok() {
                eprintln!(
                    "render path: GL shader compiled (curvature via GLSL Distort) + CPU fallback active (SDL Canvas presents; future quad can use GL)"
                );
            }
            Some(g)
        }
        Err(e) => {
            if std::env::var("DEBUG").is_ok() {
                eprintln!("render path: CPU fallback only (GL unavailable: {e}; curvature via CPU distort + stronger vignette/border)");
            }
            None
        }
    };
    let mut event_pump = sdl.event_pump().map_err(|e| e.to_string())?;
    video.text_input().start();
    let (font, font_path, pt, metrics) = setup_font_and_metrics(&ttf)?;

    // ── Resolve initial state: --story wins, else always show text menu ─
    // Auto-launch only when --story is explicitly provided. Without --story
    // we always show the catalog menu (even if stories exist locally).
    let story_path_init = if story_arg.is_some() {
        find_story(story_arg.clone())
    } else {
        None
    };

    let mut state = if let Some(sp) = story_path_init.clone() {
        let (sess, err) = match ZMachineSession::new(sp.clone()) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        };
        let mut st = AppState::new(Some(sp.clone()), err, sess);
        st.seed_banner(&font_path, pt);
        if st.vm_error.is_some() {
            if let Some(e) = &st.vm_error {
                st.grid
                    .put_str(&format!("\n !! Z-machine spawn failed: {e}\n\n"));
            }
        }
        st
    } else if story_arg.is_some() {
        // Should have errored above, but keep a visible error screen
        let mut st = AppState::new(None, Some("story not found".to_string()), None);
        st.seed_banner(&font_path, pt);
        st.grid.put_str("\n !! story file not found\n");
        st
    } else {
        // No --story → always show pure-text menu in the CRT grid
        let entries = catalog::discover();
        let menu = MenuState::new(entries);
        let st = AppState::new_with_menu(menu);
        let _ = font_path;
        let _ = pt;
        if std::env::var("DEBUG").is_ok() {
            eprintln!(
                "showing text menu ({} entries)",
                st.menu.as_ref().map_or(0, |m| m.entries.len())
            );
        }
        st
    };
    if state.vm_error.is_some() && state.session.is_none() && state.menu.is_none() {
        if let Some(e) = state.vm_error.clone() {
            if !e.contains("not found") {
                state.grid.put_str(&format!("\n !! {e}\n"));
            }
        }
    }

    run_event_loop(&video, &mut canvas, &font, &metrics, &mut state, &mut event_pump)
}

fn run_event_loop(
    video: &sdl2::VideoSubsystem,
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    font: &sdl2::ttf::Font<'_, '_>,
    metrics: &GridMetrics,
    state: &mut AppState,
    event_pump: &mut sdl2::EventPump,
) -> Result<(), String> {
    video.text_input().start();

    let mut running = true;
    while running {
        running = pump_events(event_pump, state, video, canvas);
        if !running {
            break;
        }

        poll_session(state);

        update_blink(state);

        render_frame(canvas, font, metrics, state)?;

        thread::sleep(Duration::from_millis(16));
    }

    // Drop session — closes channel and lets VM thread exit
    let _ = state.session.take();
    Ok(())
}

fn pump_events(
    event_pump: &mut sdl2::EventPump,
    state: &mut AppState,
    video: &sdl2::VideoSubsystem,
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
) -> bool {
    while let Some(event) = event_pump.poll_event() {
        // Menu mode: pure text menu, no modern GUI (no overlay modal)
        if state.is_menu_active() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return false,
                Event::KeyDown {
                    keycode: Some(kc),
                    ..
                } => {
                    // F1 in menu: refresh rather than open file picker (no rfd in menu)
                    if kc == Keycode::F1 {
                        if let Some(menu) = state.menu.as_mut() {
                            menu.refresh();
                            let m = state.menu.as_ref().unwrap();
                            m.render_to_grid(&mut state.grid);
                        }
                    } else {
                        let should_quit = state.handle_menu_key(kc);
                        if should_quit {
                            return false;
                        }
                    }
                }
                Event::TextInput { text, .. } => {
                    state.handle_menu_text(&text);
                    // handle_menu_text may have quit-worthy? check q via text is inside, but also handle digit launch
                    // If menu disappeared (launched), continue to game handling
                }
                Event::MouseButtonDown { x, y, mouse_btn: sdl2::mouse::MouseButton::Left, .. } => {
                    if controls::handle_click(&mut state.control_state, x, y) {
                        // bezel controls still work in menu
                    }
                }
                Event::MouseMotion { x, y, .. } => {
                    state.mouse_pos = Some((x, y));
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Quit { .. } => return false,
            Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => {
                state.return_to_menu("Returned to menu");
            }
            Event::KeyDown {
                keycode: Some(Keycode::F1),
                ..
            } => handle_f1_picker(state, video, canvas, event_pump),
            Event::KeyDown {
                keycode: Some(Keycode::Return | Keycode::KpEnter),
                ..
            } => state.submit_input(video, canvas, event_pump),
            Event::KeyDown {
                keycode: Some(Keycode::Backspace),
                ..
            } => state.handle_backspace(),
            Event::KeyDown {
                keycode: Some(Keycode::Up),
                ..
            } => state.history_prev(),
            Event::KeyDown {
                keycode: Some(Keycode::Down),
                ..
            } => state.history_next(),
            Event::TextInput { text, .. } => state.handle_text_input(&text),
            Event::MouseButtonDown { x, y, mouse_btn: sdl2::mouse::MouseButton::Left, .. } => {
                if controls::handle_click(&mut state.control_state, x, y) {
                    // handled — no further action
                }
            }
            Event::MouseMotion { x, y, .. } => {
                state.mouse_pos = Some((x, y));
            }
            _ => {}
        }
    }
    true
}

fn poll_session(state: &mut AppState) {
    if state.is_menu_active() {
        state.poll_menu_download();
        return;
    }
    let had_session = state.session.is_some();
    state.poll_zmachine();
    // poll_zmachine returns to menu on disconnect, so check if we already transitioned
    if state.is_menu_active() {
        return;
    }
    if had_session && state.session.is_none() {
        // Fallback: session ended without poll_zmachine handling (e.g. empty)
        state.return_to_menu("Game ended — Returned to menu");
        return;
    }
    if state.check_session_exit() {
        state.return_to_menu("Game ended — Returned to menu");
    }
}

fn update_blink(state: &mut AppState) {
    if state.last_blink.elapsed() >= Duration::from_millis(500) {
        state.blink_on = !state.blink_on;
        state.last_blink = Instant::now();
    }
}

fn render_frame(
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    font: &sdl2::ttf::Font<'_, '_>,
    metrics: &GridMetrics,
    state: &AppState,
) -> Result<(), String> {
    let t = state.start_time.elapsed().as_secs_f32();
    let raw_flicker = (t * 7.3).sin() * 0.5_f32 + 0.5_f32;
    let raw_flicker = raw_flicker * 0.04_f32;
    let hum = (t * 60.0).sin() * 0.02_f32;
    let hum = hum.abs();

    draw_bezel(canvas);
    draw_bottom_controls(canvas, &state.control_state, state.mouse_pos);
    draw_bottom_control_labels(canvas, font, &state.control_state);
    draw_power_led(canvas, state.session.is_some());
    let (glass_x, glass_y, glass_w, glass_h) = draw_glass(canvas, metrics);

    draw_grid_text_with_controls(
        canvas,
        &state.grid,
        font,
        metrics,
        raw_flicker,
        hum,
        state.blink_on,
        state.session.is_some(),
        &state.control_state,
    )?;

    let has_error = state.vm_error.is_some();
    draw_scanlines_and_vignette_with_state(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        t,
        raw_flicker,
        state.session.is_some(),
        has_error,
        &state.control_state,
    );

    let _ = f32_to_u8_clamped;
    let _ = u32_to_i32(0);
    let _ = usize_to_i32(0);

    canvas.present();
    Ok(())
}
