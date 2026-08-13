mod backend;
mod cli;
mod constants;
mod font;
mod grid;
mod render;

use std::collections::VecDeque;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::{Duration, Instant};

use sdl2::event::Event;
use sdl2::keyboard::Keycode;

use backend::{find_dfrotz, find_story, spawn_dfrotz, DfrotzSession};
use constants::{
    f32_to_u8_clamped, u32_to_i32, usize_to_i32, BEZEL, INNER_PAD, WINDOW_H, WINDOW_W,
};
use grid::Grid;
use render::{
    compute_grid_metrics, draw_bezel, draw_glass, draw_grid_text, draw_power_led,
    draw_scanlines_and_vignette, GridMetrics,
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

struct AppState {
    grid: Grid,
    input_buf: String,
    input_history: VecDeque<String>,
    history_idx: Option<usize>,
    session: Option<DfrotzSession>,
    dfrotz_error: Option<String>,
    story_path: Option<PathBuf>,
    blink_on: bool,
    last_blink: Instant,
    start_time: Instant,
}

impl AppState {
    fn new(
        story_path: Option<PathBuf>,
        dfrotz_error: Option<String>,
        session: Option<DfrotzSession>,
    ) -> Self {
        Self {
            grid: Grid::new(),
            input_buf: String::new(),
            input_history: VecDeque::new(),
            history_idx: None,
            session,
            dfrotz_error,
            story_path,
            blink_on: true,
            last_blink: Instant::now(),
            start_time: Instant::now(),
        }
    }

    fn seed_banner(&mut self, font_path: &Path, pt: u16, dfrotz_path: Option<PathBuf>) {
        self.grid
            .put_str(" ZORK CRT  •  SDL2 phosphor  •  80×24  •  VT323\n");
        self.grid
            .put_str(&format!(" font: {} @ {pt}pt\n", font_path.display()));
        if let Some(sp) = &self.story_path {
            self.grid.put_str(&format!(" story: {}\n", sp.display()));
        } else {
            self.grid
                .put_str(" story: (none) — pass --story <path> or pick file (F1)\n");
        }
        if let Some(dp) = dfrotz_path {
            self.grid.put_str(&format!(" dfrotz: {}\n", dp.display()));
        } else {
            self.grid
                .put_str(" dfrotz: NOT FOUND — brew install frotz\n");
        }
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
                let to_send = format!("{line}\n");
                if let Err(e) = sess.stdin.write_all(to_send.as_bytes()) {
                    self.grid
                        .put_str(&format!("\n !! write to dfrotz failed: {e}\n"));
                } else {
                    let _ = sess.stdin.flush();
                }
            }
            self.input_buf.clear();
        } else if self.story_path.is_none() {
            let picked = rfd::FileDialog::new()
                .add_filter("Z-machine", &["z3", "z5", "z8", "zip"])
                .pick_file();
            restore_focus(video, canvas, event_pump);
            if let Some(p) = picked {
                let dfrotz = find_dfrotz();
                if let Some(dp) = dfrotz {
                    match spawn_dfrotz(&dp, &p) {
                        Ok(s) => {
                            self.grid.clear();
                            self.grid.put_str(&format!(" [picked {}]\n\n", p.display()));
                            self.session = Some(s);
                            self.story_path = Some(p);
                            self.dfrotz_error = None;
                        }
                        Err(e) => self.grid.put_str(&format!("\n !! {e}\n")),
                    }
                }
            }
        } else {
            self.grid.newline();
            self.input_buf.clear();
        }
    }

    fn poll_dfrotz(&mut self) {
        let Some(sess) = self.session.as_mut() else {
            return;
        };
        let mut disconnected = false;
        loop {
            match sess.rx.try_recv() {
                Ok(chunk) => self.grid.put_str(&chunk),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            let status = sess.child.try_wait().ok().flatten();
            self.grid.put_str(&format!(
                "\n\n [dfrotz exited{} — press Esc to quit, F1 to load another story]\n",
                status.map(|s| format!(" status={s}")).unwrap_or_default()
            ));
            self.session = None;
            return;
        }
        if let Ok(Some(_status)) = sess.child.try_wait() {
            let mut had_more = false;
            loop {
                match sess.rx.try_recv() {
                    Ok(c) => {
                        self.grid.put_str(&c);
                        had_more = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.session = None;
                        return;
                    }
                }
            }
            if !had_more {
                self.grid
                    .put_str("\n [dfrotz exited — press Esc to quit, F1 to load another story]\n");
            }
        }
    }

    fn check_session_exit(&mut self) -> bool {
        if let Some(sess) = self.session.as_mut() {
            if let Ok(Some(_)) = sess.child.try_wait() {
                match sess.rx.try_recv() {
                    Ok(c) => {
                        self.grid.put_str(&c);
                        return false;
                    }
                    Err(TryRecvError::Empty) => {
                        return true;
                    }
                    Err(TryRecvError::Disconnected) => return true,
                }
            }
        }
        false
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
    if let Some(mut s) = state.session.take() {
        let _ = s.child.kill();
    }
    let Some(dp) = find_dfrotz() else {
        state
            .grid
            .put_str("\n !! dfrotz still not found — brew install frotz\n");
        return;
    };
    match spawn_dfrotz(&dp, &p) {
        Ok(s) => {
            state.grid.clear();
            state
                .grid
                .put_str(&format!(" [picked {} — spawning dfrotz]\n\n", p.display()));
            state.session = Some(s);
            state.story_path = Some(p);
            state.dfrotz_error = None;
            state.input_buf.clear();
        }
        Err(e) => {
            state.grid.put_str(&format!("\n !! spawn failed: {e}\n"));
            state.dfrotz_error = Some(e);
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
    let window = video
        .window(
            "ZORK I — CRT  •  80×24  •  VT323 phosphor",
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
    println!(
        "font: {} @ {pt}pt  cell≈{}×{}  grid={}×{}  inner={}×{}",
        font_path.display(),
        cell_w,
        cell_h,
        grid_w,
        grid_h,
        WINDOW_W,
        WINDOW_H
    );
    let metrics = compute_grid_metrics(INNER_PAD, cell_w, cell_h);
    Ok((font, font_path, pt, metrics))
}

fn main() -> Result<(), String> {
    let cli = cli::parse_args()?;
    if cli.show_help {
        cli::print_help();
        return Ok(());
    }
    if cli.show_version {
        println!("zork-crt-gui 0.1.0 (SDL2 CRT, dfrotz backend)");
        return Ok(());
    }

    let story_arg = cli.story_arg.clone();
    if story_arg.is_some() && find_story(story_arg.clone()).is_none() {
        return Err(format!(
            "Story file not found: {:?}. Pass --story <path> with existing file or place story at assets/stories/zork1.z3.",
            story_arg.expect("story_arg some")
        ));
    }
    let dfrotz_path = find_dfrotz();
    let (sdl, video, ttf) = init_sdl()?;
    let mut canvas = create_window(&video)?;
    let mut event_pump = sdl.event_pump().map_err(|e| e.to_string())?;
    // Ensure text input is active before any picker steals focus.
    video.text_input().start();
    // Window now exists, so the initial file picker can restore SDL focus afterwards.
    // This fixes the startup focus bug where keyboard stayed in the launching terminal.
    let story_path_init = resolve_story_with_window(story_arg.as_ref(), &video, &mut canvas, &mut event_pump);
    // Re-assert text input after startup picker (restore_focus already does, but ensure).
    video.text_input().start();
    let (font, font_path, pt, metrics) = setup_font_and_metrics(&ttf)?;

    let (initial_session, initial_error) = if let (Some(dp), Some(sp)) =
        (&dfrotz_path, &story_path_init)
    {
        match spawn_dfrotz(dp, sp) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        let err = if dfrotz_path.is_none() {
            Some("dfrotz not found. Install: brew install frotz (looks at /opt/homebrew/bin/dfrotz, /usr/local/bin/dfrotz, $PATH).".to_string())
        } else {
            None
        };
        (None, err)
    };

    let mut state = AppState::new(story_path_init, initial_error, initial_session);
    state.seed_banner(&font_path, pt, dfrotz_path.clone());
    if let (Some(dp), Some(sp)) = (&dfrotz_path, &state.story_path) {
        if state.session.is_some() {
            state.grid.put_str(&format!(
                "\n [spawning dfrotz {} -w 80 -h 24 -m -p {}]\n\n",
                dp.display(),
                sp.display()
            ));
        } else if let Some(e) = &state.dfrotz_error {
            state
                .grid
                .put_str(&format!("\n !! dfrotz spawn failed: {e}\n\n"));
        }
    } else {
        if dfrotz_path.is_none() {
            state
                .grid
                .put_str("\n !! dfrotz not found — install with: brew install frotz\n");
            state
                .grid
                .put_str("    then re-run: cargo run -- --story <path>\n\n");
        }
        if state.story_path.is_none() {
            state
                .grid
                .put_str("\n !! no story file — pass --story <path> or use file picker\n");
            state
                .grid
                .put_str("    e.g. cargo run -- --story ./zork1.z3\n");
            state
                .grid
                .put_str("        cargo run -- --story assets/stories/zork1.z3\n");
            state.grid.put_str(
                "    or drag a .z3/.z5/.z8/.zip onto the window (not yet) — use picker via rfd\n\n",
            );
        }
    }
    if state.dfrotz_error.is_some() && state.session.is_none() && dfrotz_path.is_some() {
        if let Some(e) = state.dfrotz_error.clone() {
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

    if let Some(mut s) = state.session.take() {
        let _ = s.child.kill();
        let _ = s.child.wait();
    }
    Ok(())
}

fn pump_events(
    event_pump: &mut sdl2::EventPump,
    state: &mut AppState,
    video: &sdl2::VideoSubsystem,
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
) -> bool {
    while let Some(event) = event_pump.poll_event() {
        match event {
            Event::Quit { .. }
            | Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => return false,
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
            _ => {}
        }
    }
    true
}

fn poll_session(state: &mut AppState) {
    let had_session = state.session.is_some();
    state.poll_dfrotz();
    if had_session && state.session.is_none() {
        return;
    }
    if state.check_session_exit() {
        if let Some(mut s) = state.session.take() {
            let _ = s.child.try_wait();
        }
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
    let flicker = (t * 7.3).sin() * 0.5_f32 + 0.5_f32;
    let flicker = flicker * 0.04_f32;
    let hum = (t * 60.0).sin() * 0.02_f32;
    let hum = hum.abs();

    draw_bezel(canvas);
    draw_power_led(canvas, state.session.is_some());
    let (glass_x, glass_y, glass_w, glass_h) = draw_glass(canvas, metrics);

    draw_grid_text(
        canvas,
        &state.grid,
        font,
        metrics,
        flicker,
        hum,
        state.blink_on,
        state.session.is_some(),
    )?;

    let has_error = state.dfrotz_error.is_some();
    draw_scanlines_and_vignette(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        t,
        flicker,
        state.session.is_some(),
        has_error,
    );

    let _ = f32_to_u8_clamped;
    let _ = u32_to_i32(0);
    let _ = usize_to_i32(0);

    canvas.present();
    Ok(())
}
