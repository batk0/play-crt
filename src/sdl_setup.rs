#![allow(clippy::pedantic)]

use std::path::PathBuf;

use crate::constants::{BEZEL, INNER_PAD};
use crate::font;
use crate::render::{compute_grid_metrics, GridMetrics};

pub fn init_sdl() -> Result<(sdl2::Sdl, sdl2::VideoSubsystem, sdl2::ttf::Sdl2TtfContext), String> {
    let sdl = sdl2::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let ttf = sdl2::ttf::init().map_err(|e| e.to_string())?;
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");
    Ok((sdl, video, ttf))
}

/// Initialize SDL audio subsystem. Returns `None` on failure (graceful fallback).
#[must_use]
pub fn init_audio(sdl: &sdl2::Sdl) -> Option<sdl2::AudioSubsystem> {
    match sdl.audio() {
        Ok(a) => Some(a),
        Err(e) => {
            if std::env::var("DEBUG").is_ok() {
                eprintln!("audio init failed: {e} (running silent)");
            }
            None
        }
    }
}

pub fn create_window(
    video: &sdl2::VideoSubsystem,
) -> Result<sdl2::render::Canvas<sdl2::video::Window>, String> {
    use crate::constants::{WINDOW_H, WINDOW_W};
    // Request GL 3.3 core for the optional PUBLIC DOMAIN CRT shader path (crt-lottes via glow).
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
            "PLAY-CRT  •  80×24  •  VT323 phosphor (CRT)",
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

/// Update the window title to reflect the active game.
/// Call after launching a game: e.g. `update_window_title(&mut canvas, Some("Mini-Zork"))`.
/// Passing `None` resets to the neutral `"PLAY-CRT"` title.
pub fn update_window_title(
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    game_title: Option<&str>,
) {
    let title = match game_title {
        Some(name) if !name.trim().is_empty() => format!("PLAY-CRT — {}", name.trim()),
        _ => "PLAY-CRT".to_string(),
    };
    let _ = canvas.window_mut().set_title(&title);
}

pub fn setup_font_and_metrics(
    ttf: &sdl2::ttf::Sdl2TtfContext,
) -> Result<(sdl2::ttf::Font<'_, 'static>, PathBuf, u16, GridMetrics), String> {
    let grid_w = u32::try_from(crate::constants::window_w_i32() - BEZEL * 2 - INNER_PAD * 2)
        .expect("grid_w positive");
    let grid_h = u32::try_from(crate::constants::window_h_i32() - BEZEL * 2 - INNER_PAD * 2)
        .expect("grid_h positive");

    let (mut font, font_path, pt) = font::choose_font(ttf, grid_w, grid_h)?;
    font.set_style(sdl2::ttf::FontStyle::NORMAL);
    font.set_hinting(sdl2::ttf::Hinting::Light);

    let (cell_w, cell_h) = {
        let sample = "M".repeat(crate::constants::COLS);
        let (w, _) = font.size_of(&sample).unwrap_or((grid_w, 20));
        let cw = w / crate::constants::cols_u32();
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
            crate::constants::WINDOW_W,
            crate::constants::WINDOW_H
        );
    }
    let metrics = compute_grid_metrics(INNER_PAD, cell_w, cell_h);
    Ok((font, font_path, pt, metrics))
}
