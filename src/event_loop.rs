use std::thread;
use std::time::{Duration, Instant};

use sdl2::event::Event;
use sdl2::keyboard::Keycode;

use crate::app::AppState;
use crate::constants::{f32_to_u8_clamped, u32_to_i32, usize_to_i32};
use crate::controls;
use crate::render::{
    draw_bezel, draw_bottom_control_labels, draw_bottom_controls, draw_glass,
    draw_grid_text_with_controls, draw_power_led, draw_scanlines_and_vignette_with_state,
    GridMetrics,
};

pub fn run_event_loop(
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
        running = pump_events(event_pump, state);
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

pub(crate) fn pump_events(
    event_pump: &mut sdl2::EventPump,
    state: &mut AppState,
) -> bool {
    while let Some(event) = event_pump.poll_event() {
        // Menu mode: pure text menu, no modal overlay
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
                    let should_quit = state.handle_menu_key(kc);
                    if should_quit {
                        return false;
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
                keycode: Some(Keycode::Return | Keycode::KpEnter),
                ..
            } => state.submit_input(),
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

pub(crate) fn poll_session(state: &mut AppState) {
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

pub(crate) fn update_blink(state: &mut AppState) {
    if state.last_blink.elapsed() >= Duration::from_millis(500) {
        state.blink_on = !state.blink_on;
        state.last_blink = Instant::now();
    }
}

pub(crate) fn render_frame(
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
