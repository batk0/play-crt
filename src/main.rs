#![allow(clippy::pedantic)]
#![allow(clippy::nursery)]

mod app;
mod backend;
mod basic;
mod basic_catalog;
mod catalog;
mod cli;
mod config;
mod constants;
mod controls;
mod crt_gl;
mod crt_pi;
mod download;
mod event_loop;
mod font;
mod grid;
mod menu;
mod paths;
mod render;
mod saves;
mod sdl_setup;
mod slot_menu;
mod zmachine;

use app::AppState;
use backend::{find_story, ZMachineSession};
use menu::MenuState;

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
    if let Some(arg) = story_arg.as_ref() {
        if find_story(Some(arg.clone())).is_none() {
            return Err(format!(
                "Story file not found: {:?}. Pass --story <path> with existing file or use --story with a path under {}.",
                arg,
                paths::stories_dir().display()
            ));
        }
    }

    // Ensure data layout exists early (creates stories/downloads/saves dirs)
    let _ = paths::ensure_layout();

    let (sdl, video, ttf) = sdl_setup::init_sdl()?;
    let mut canvas = sdl_setup::create_window(&video)?;
    // Optional PUBLIC DOMAIN CRT GL path (crt-lottes): compile the shader at startup and keep it alive.
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
    let (font, font_path, pt, metrics) = sdl_setup::setup_font_and_metrics(&ttf)?;

    // ── Resolve initial state: --story wins, else always show text menu ─
    // Auto-launch only when --story is explicitly provided. Without --story
    // we always show the catalog menu (even if stories exist locally).
    let story_path_init = if story_arg.is_some() {
        find_story(story_arg.clone())
    } else {
        None
    };

    let mut state = if let Some(sp) = story_path_init.clone() {
        // When --story is used, launch with slot 1 auto-selected so SAVE/RESTORE
        // still use the slot system (no slot picker for CLI launch).
        let gid = paths::game_id_for_path(&sp);
        let (sess, err) = match ZMachineSession::new_with_slot(sp.clone(), gid, 1) {
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
        // No --story → show pure-text menu, restoring last catalog kind if persisted
        let initial_kind = config::load_last_catalog().unwrap_or(menu::CatalogKind::ZMachine);
        let menu = MenuState::new_for_kind(initial_kind);
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
    if state.vm_error.is_some()
        && state.backend.is_none()
        && state.menu.is_none()
        && story_path_init.is_none()
    {
        if let Some(e) = state.vm_error.clone() {
            if !e.contains("not found") {
                state.grid.put_str(&format!("\n !! {e}\n"));
            }
        }
    }

    event_loop::run_event_loop(&video, &mut canvas, &font, &metrics, &mut state, &mut event_pump)
}
