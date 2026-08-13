//! CRT rendering — bezel, glass, phosphor text and **crt-pi** scanlines.
//!
//! The visual style is driven by `assets/shaders/crt-pi.glsl` (GPL-2.0+,
//! © 2015-2016 davej, from `libretro/glsl-shaders`). That shader is the
//! reference implementation; this file contains:
//! - the SDL2 CPU fallback that ports its math via `crate::crt_pi`
//!   (`CalcScanLine`, gamma, bloom, mask, barrel distortion approximation)
//! - the bezel/glass/bloom text that is unchanged regardless of path
//!
//! When an OpenGL context is available, `crate::crt_gl::CrtGl` compiles the
//! same GLSL verbatim and can be used for a fullscreen-quad post-process
//! (see `crt_gl.rs`). The CPU path below therefore stays pixel-identical to
//! the shader's intent and is the fallback when GL is not available.

use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::Canvas;
use sdl2::video::Window;

use crate::constants::{
    f32_to_u8_clamped, i32_to_f32, u32_to_i32, usize_to_i32, window_h_i32, window_w_i32, BEZEL,
    BEZEL_COLOR, BEZEL_HILITE, BEZEL_SHADOW, GLASS_BG, PHOSPHOR, PHOSPHOR_BLOOM, PHOSPHOR_DIM, ROWS,
    WINDOW_H, WINDOW_W,
};
use crate::crt_pi;
use crate::grid::Grid;

// grid_x/y/w/h are computed for completeness; used indirectly via origin and glass rect.
// Allow dead_code to minimize warnings while keeping struct stable for future use.
#[allow(dead_code)]
pub struct GridMetrics {
    pub grid_x: i32,
    pub grid_y: i32,
    pub grid_w: u32,
    pub grid_h: u32,
    pub cell_w: u32,
    pub cell_h: u32,
    pub origin_x: i32,
    pub origin_y: i32,
}

// Allow similar_names: w/h, grid_w/grid_h pairs are conventional and distinct
#[allow(clippy::similar_names)]
pub fn compute_grid_metrics(inner_pad: i32, cell_w: u32, cell_h: u32) -> GridMetrics {
    let grid_x = BEZEL + inner_pad;
    let grid_y = BEZEL + inner_pad;
    let win_w = window_w_i32();
    let win_h = window_h_i32();
    let grid_w = u32::try_from(win_w - BEZEL * 2 - inner_pad * 2).expect("grid_w positive");
    let grid_h = u32::try_from(win_h - BEZEL * 2 - inner_pad * 2).expect("grid_h positive");

    let cols_u32 = crate::constants::cols_u32();
    let rows_u32 = crate::constants::rows_u32();
    let total_text_w = cell_w.checked_mul(cols_u32).unwrap_or(cell_w);
    let total_text_h = cell_h.checked_mul(rows_u32).unwrap_or(cell_h);
    let total_w = u32_to_i32(total_text_w);
    let total_h = u32_to_i32(total_text_h);
    let gw = u32_to_i32(grid_w);
    let gh = u32_to_i32(grid_h);
    let offset_x = ((gw - total_w) / 2).max(0);
    let offset_y = ((gh - total_h) / 2).max(0);
    let origin_x = grid_x + offset_x;
    let origin_y = grid_y + offset_y;

    GridMetrics {
        grid_x,
        grid_y,
        grid_w,
        grid_h,
        cell_w,
        cell_h,
        origin_x,
        origin_y,
    }
}

pub fn draw_bezel(canvas: &mut Canvas<Window>) {
    canvas.set_draw_color(BEZEL_COLOR);
    canvas.clear();

    canvas.set_draw_color(BEZEL_HILITE);
    let _ = canvas.fill_rect(Rect::new(0, 0, WINDOW_W, 4));
    let _ = canvas.fill_rect(Rect::new(0, 0, 4, WINDOW_H));
    canvas.set_draw_color(BEZEL_SHADOW);
    let wh_minus_4 = u32_to_i32(WINDOW_W - 4);
    let hh_minus_4 = u32_to_i32(WINDOW_H - 4);
    let _ = canvas.fill_rect(Rect::new(0, hh_minus_4, WINDOW_W, 4));
    let _ = canvas.fill_rect(Rect::new(wh_minus_4, 0, 4, WINDOW_H));

    draw_screws(canvas);
    draw_speaker_grille(canvas);
    draw_brand_plate(canvas);
}

fn draw_screws(canvas: &mut Canvas<Window>) {
    let screw_color = Color::RGB(0x1e, 0x1e, 0x1e);
    let screw_hilite = Color::RGB(0x5a, 0x5a, 0x56);
    let ww = window_w_i32();
    let wh = window_h_i32();
    let screw_positions = [
        (18, 18),
        (ww - 18, 18),
        (18, wh - 18),
        (ww - 18, wh - 18),
        (ww / 2, 18),
        (ww / 2, wh - 18),
    ];
    for (sx, sy) in screw_positions {
        canvas.set_draw_color(screw_color);
        let _ = canvas.fill_rect(Rect::new(sx - 6, sy - 6, 12, 12));
        canvas.set_draw_color(screw_hilite);
        let _ = canvas.draw_rect(Rect::new(sx - 6, sy - 6, 12, 12));
        canvas.set_draw_color(Color::RGB(0x3a, 0x3a, 0x36));
        let _ = canvas.draw_line((sx - 4, sy), (sx + 4, sy));
        let _ = canvas.draw_line((sx, sy - 4), (sx, sy + 4));
    }
}

fn draw_speaker_grille(canvas: &mut Canvas<Window>) {
    let wh = window_h_i32();
    let grille_y = wh - BEZEL + 12;
    let grille_x = BEZEL + 24;
    let grille_w: i32 = 160;
    let grille_h: i32 = BEZEL - 24;
    canvas.set_draw_color(Color::RGB(0x1a, 0x1a, 0x18));
    let _ = canvas.fill_rect(Rect::new(
        grille_x,
        grille_y,
        u32::try_from(grille_w).expect("grille_w positive"),
        u32::try_from(grille_h).expect("grille_h positive"),
    ));
    canvas.set_draw_color(Color::RGB(0x0f, 0x0f, 0x0e));
    for i in 0..6 {
        let y = grille_y + 4 + i * 4;
        let _ = canvas.draw_line((grille_x + 6, y), (grille_x + grille_w - 6, y));
    }
}

fn draw_brand_plate(canvas: &mut Canvas<Window>) {
    let ww = window_w_i32();
    let plate_x = ww / 2 - 90;
    let plate_y = 10;
    canvas.set_draw_color(Color::RGB(0x3a, 0x3a, 0x36));
    let _ = canvas.fill_rect(Rect::new(plate_x, plate_y, 180, 22));
    canvas.set_draw_color(BEZEL_HILITE);
    let _ = canvas.draw_rect(Rect::new(plate_x, plate_y, 180, 22));
}

pub fn draw_power_led(canvas: &mut Canvas<Window>, session_active: bool) {
    let wh = window_h_i32();
    let led_x = window_w_i32() - BEZEL - 28;
    let led_y = wh - BEZEL + 18;
    let led_color = if session_active {
        Color::RGB(0xff, 0x33, 0x33)
    } else {
        Color::RGB(0x44, 0x11, 0x11)
    };
    canvas.set_draw_color(led_color);
    let _ = canvas.fill_rect(Rect::new(led_x, led_y, 10, 10));
    if session_active {
        canvas.set_draw_color(Color {
            r: 0xff,
            g: 0x33,
            b: 0x33,
            a: 80,
        });
        let _ = canvas.fill_rect(Rect::new(led_x - 2, led_y - 2, 14, 14));
        canvas.set_draw_color(led_color);
        let _ = canvas.fill_rect(Rect::new(led_x, led_y, 10, 10));
    }
}

pub fn draw_glass(canvas: &mut Canvas<Window>, metrics: &GridMetrics) -> (i32, i32, i32, i32) {
    let glass_x = BEZEL;
    let glass_y = BEZEL;
    let glass_w = window_w_i32() - BEZEL * 2;
    let glass_h = window_h_i32() - BEZEL * 2;
    canvas.set_draw_color(GLASS_BG);
    let _ = canvas.fill_rect(Rect::new(
        glass_x,
        glass_y,
        u32::try_from(glass_w).expect("glass_w positive"),
        u32::try_from(glass_h).expect("glass_h positive"),
    ));
    // Curved bezel border — reflects barrel distortion by using a slightly
    // darker inner frame plus the vignette/corners that inset at top/bottom.
    canvas.set_draw_color(Color::RGB(0x05, 0x0a, 0x06));
    let _ = canvas.draw_rect(Rect::new(
        glass_x,
        glass_y,
        u32::try_from(glass_w).expect("glass_w positive"),
        u32::try_from(glass_h).expect("glass_h positive"),
    ));
    canvas.set_draw_color(Color::RGB(0x14, 0x2a, 0x18));
    let _ = canvas.draw_rect(Rect::new(
        glass_x + 1,
        glass_y + 1,
        u32::try_from(glass_w - 2).expect("glass_w-2 positive"),
        u32::try_from(glass_h - 2).expect("glass_h-2 positive"),
    ));
    let _ = metrics;
    (glass_x, glass_y, glass_w, glass_h)
}

/// Compute a barrel inset for a given `y` row inside the glass.
/// Used to bow scanlines and inset the text so the screen looks bulged
/// (narrower at top/bottom, widest at midline).
fn barrel_inset_for_y(y: i32, glass_y: i32, glass_h: i32) -> i32 {
    let params = crt_pi::CrtPiParams::default();
    let gf = i32_to_f32(glass_h);
    let yf = i32_to_f32(y);
    let gy = i32_to_f32(glass_y);
    if gf <= 0.0 {
        return 0;
    }
    let ny = (yf - gy) / gf - 0.5;
    let rsq = ny * ny;
    // Inset grows quadratically away from centerline; scaled so that
    // CURVATURE 0.20 gives ~12–16 px inset at extremes on a 764px glass.
    let inset_f = rsq * params.curvature_x * 140.0 + rsq * rsq * params.curvature_x * 60.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    {
        inset_f.round().clamp(0.0, 28.0) as i32
    }
}

// Too many arguments is pedantic but grid rendering needs these; allow with justification
#[allow(clippy::too_many_arguments)]
pub fn draw_grid_text(
    canvas: &mut Canvas<Window>,
    grid: &Grid,
    font: &sdl2::ttf::Font<'_, '_>,
    metrics: &GridMetrics,
    flicker: f32,
    hum: f32,
    blink_on: bool,
    session_active: bool,
) -> Result<(), String> {
    draw_grid_text_with_params(
        canvas,
        grid,
        font,
        metrics,
        flicker,
        hum,
        blink_on,
        session_active,
        crt_pi::CrtPiParams::default(),
    )
}

/// Variant that accepts explicit `CrtPiParams` (curvature override via `--curvature`).
#[allow(clippy::too_many_arguments)]
pub fn draw_grid_text_with_params(
    canvas: &mut Canvas<Window>,
    grid: &Grid,
    font: &sdl2::ttf::Font<'_, '_>,
    metrics: &GridMetrics,
    flicker: f32,
    hum: f32,
    blink_on: bool,
    session_active: bool,
    params: crt_pi::CrtPiParams,
) -> Result<(), String> {
    let texture_creator = canvas.texture_creator();
    let cell_h = u32_to_i32(metrics.cell_h);
    let cell_w = u32_to_i32(metrics.cell_w);
    let glass_x = BEZEL;
    let glass_y = BEZEL;
    let glass_w = window_w_i32() - BEZEL * 2;
    let glass_h = window_h_i32() - BEZEL * 2;

    for row in 0..ROWS {
        let line = grid.line_trimmed(row);
        let row_i = usize_to_i32(row);
        if line.is_empty() && !(row == grid.cursor_y && blink_on) {
            continue;
        }
        let y_px = metrics.origin_y + row_i * cell_h;
        // Apply barrel warp to the row's origin so the whole tube bulges.
        // Center of the line is distorted; the delta is applied to the dst rect.
        let center_y = i32_to_f32(y_px) + i32_to_f32(cell_h) * 0.5;
        let center_x = i32_to_f32(metrics.origin_x);
        let (dx, _dy) = if let Some((dx_f, _)) = crt_pi::distort_point(
            center_x,
            center_y,
            i32_to_f32(glass_x),
            i32_to_f32(glass_y),
            i32_to_f32(glass_w),
            i32_to_f32(glass_h),
            &params,
        ) {
            (dx_f - center_x, 0.0)
        } else {
            // Out-of-bounds due to extreme curvature — clamp to slight inset
            let inset = barrel_inset_for_y(y_px, glass_y, glass_h);
            ( -i32_to_f32(inset) * 0.5, 0.0)
        };
        // Dampen dx so text stays readable — divide by 1.4 and clamp to ±10px.
        // NOTE: crt-pi's Distort() is screen->texture (outward) for sampling;
        // for forward geometry we need the inverse, hence `-dx` to get barrel
        // bulge (wider at middle, narrower at top/bottom). Previous `+dx` gave
        // pincushion (narrow middle, wide top/bottom).
        #[allow(clippy::cast_possible_truncation)]
        let warped_origin_x = metrics.origin_x - (dx / 1.4).clamp(-10.0, 10.0) as i32;

        if !line.is_empty() {
            render_line_with_bloom(
                canvas,
                font,
                &texture_creator,
                &line,
                warped_origin_x,
                y_px,
                metrics.cell_h,
                flicker,
                hum,
            )?;
        }

        if row == grid.cursor_y && blink_on {
            draw_cursor(
                canvas,
                warped_origin_x,
                y_px,
                grid.cursor_x,
                cell_w,
                metrics.cell_w,
                metrics.cell_h,
                session_active,
            );
        }
    }
    Ok(())
}

// Rendering needs canvas, font, position and CRT flicker; grouping would obscure intent.
// Allow too_many_arguments with justification: bloom rendering is inherently multi-param.
#[allow(clippy::too_many_arguments)]
fn render_line_with_bloom(
    canvas: &mut Canvas<Window>,
    font: &sdl2::ttf::Font<'_, '_>,
    texture_creator: &sdl2::render::TextureCreator<sdl2::video::WindowContext>,
    line: &str,
    origin_x: i32,
    y_px: i32,
    cell_h: u32,
    flicker: f32,
    hum: f32,
) -> Result<(), String> {
    // Bloom halo — matches crt-pi BLOOM_FACTOR intent: bright scanlines widen.
    // SDL cannot do per-pixel bloom, so we approximate with two offset blits in
    // PHOSPHOR_BLOOM at reduced alpha, same as before but kept for compatibility.
    let bloom_surf = font
        .render(line)
        .blended(PHOSPHOR_BLOOM)
        .map_err(|e| e.to_string())?;
    let (width, height) = font.size_of(line).unwrap_or((200, cell_h));
    let bloom_rect = Rect::new(origin_x + 1, y_px + 1, width, height);
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    let mut bloom_tex = texture_creator
        .create_texture_from_surface(&bloom_surf)
        .map_err(|e| e.to_string())?;
    bloom_tex.set_blend_mode(sdl2::render::BlendMode::Blend);
    bloom_tex.set_alpha_mod(70);
    let _ = canvas.copy(&bloom_tex, None, bloom_rect);
    let bloom_rect2 = Rect::new(origin_x, y_px, width, height);
    let mut bloom_tex2 = texture_creator
        .create_texture_from_surface(&bloom_surf)
        .map_err(|e| e.to_string())?;
    bloom_tex2.set_blend_mode(sdl2::render::BlendMode::Blend);
    bloom_tex2.set_alpha_mod(35);
    let _ = canvas.copy(&bloom_tex2, None, bloom_rect2);

    let surf = font
        .render(line)
        .blended(PHOSPHOR)
        .map_err(|e| e.to_string())?;
    let mut tex = texture_creator
        .create_texture_from_surface(&surf)
        .map_err(|e| e.to_string())?;
    tex.set_blend_mode(sdl2::render::BlendMode::Blend);
    let alpha_f = 255.0 * (1.0 - flicker * 0.5 - hum * 0.3);
    let mut alpha = f32_to_u8_clamped(alpha_f);
    if alpha < 200 {
        alpha = 200;
    }
    tex.set_alpha_mod(alpha);
    let dst = Rect::new(origin_x, y_px, width, height);
    let _ = canvas.copy(&tex, None, dst);
    Ok(())
}

// Cursor needs geometry + session state; 8 args is domain-justified for single-rect draw.
#[allow(clippy::too_many_arguments)]
fn draw_cursor(
    canvas: &mut Canvas<Window>,
    origin_x: i32,
    y_px: i32,
    cursor_x: usize,
    cell_w_i32: i32,
    cell_w: u32,
    cell_h: u32,
    session_active: bool,
) {
    let cx = origin_x + usize_to_i32(cursor_x) * cell_w_i32;
    let cy = y_px;
    let cursor_color = if session_active {
        PHOSPHOR
    } else {
        PHOSPHOR_DIM
    };
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    canvas.set_draw_color(cursor_color);
    let _ = canvas.fill_rect(Rect::new(cx, cy, cell_w, cell_h));
    canvas.set_draw_color(Color::RGB(0xee, 0xff, 0xaa));
    let _ = canvas.fill_rect(Rect::new(cx + 1, cy + 1, cell_w - 2, cell_h - 2));
}

// Decomposed CRT effects to keep each function <100 lines and avoid too_many_lines

#[allow(clippy::too_many_arguments)]
pub fn draw_scanlines_and_vignette(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    t: f32,
    flicker: f32,
    session_active: bool,
    has_error: bool,
) {
    draw_scanlines(canvas, glass_x, glass_y, glass_w, glass_h, t);
    draw_aperture_mask(canvas, glass_x, glass_y, glass_w, glass_h);
    draw_vignette(canvas, glass_x, glass_y, glass_w, glass_h);
    draw_corners(canvas, glass_x, glass_y, glass_w, glass_h);
    draw_glass_highlights(canvas, glass_x, glass_y, glass_w, glass_h, flicker);
    draw_bezel_glow(canvas, glass_x, glass_y, glass_w, glass_h);
    draw_status_bar(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        session_active,
        has_error,
    );
}

/// CPU port of crt-pi's `CalcScanLine` scanline generation.
///
/// Uses `crate::crt_pi::scanline_weight_for_row` (which mirrors
/// `CalcScanLineWeight` + `MULTISAMPLE` + `BLOOM_FACTOR`) to derive a per-row
/// darkness alpha. The result is a black overlay whose opacity is stronger in
/// the gaps between scanlines and weaker on the lines — identical curve to
/// the GLSL. Scanlines are inset via `barrel_inset_for_y` so the tube's
/// bulge is visible (shorter at top/bottom, full width at centerline).
fn draw_scanlines(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    t: f32,
) {
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    let params = crt_pi::CrtPiParams::default();
    // crt-pi filterWidth: InputSize.y≈glass_h, OutputSize.y≈WINDOW_H
    let filt = crt_pi::filter_width(i32_to_f32(glass_h), i32_to_f32(window_h_i32()));

    for y in glass_y..glass_y + glass_h {
        let weight = crt_pi::scanline_weight_for_row(y, glass_y, &params, filt);
        // weight is bloom-scaled 0.18..1.5 — invert to darkness: gap→opaque, line→transparent
        // Keep original subtle wobble (sin) as a tiny vertical phase shift, additive to weight.
        let y_f = i32_to_f32(y);
        let wobble = (t * 2.0 + y_f * 0.05).sin() * 0.03;
        let w_pm = (weight + wobble).clamp(params.scanline_gap * params.bloom_factor, params.bloom_factor);
        // Map to alpha 0..70: weight 1.5 (on-line) → ~0, weight 0.18 (gap) → ~55
        let normalized = 1.0 - (w_pm / params.bloom_factor);
        let alpha_f = normalized * 70.0;
        let a = f32_to_u8_clamped(alpha_f);
        if a < 2 {
            continue;
        }
        // Bow scanlines: inset left/right edges so the raster looks barrel-curved.
        let inset = barrel_inset_for_y(y, glass_y, glass_h);
        canvas.set_draw_color(Color::RGBA(0, 0, 0, a));
        let _ = canvas.draw_line(
            (glass_x + inset, y),
            (glass_x + glass_w - inset, y),
        );
    }
}

/// Aperture/shadow mask — mirrors `MASK_TYPE 1` / `2` in crt-pi.glsl.
///
/// The shader modulates `colour * mask` per fragment where mask alternates
/// every 2 or 3 pixels. Here we approximate with thin vertical overlays at
/// low alpha so the phosphor still dominates but a subtle stripe is visible.
fn draw_aperture_mask(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
) {
    // Match shader: MASK_TYPE 1 (green/magenta) is the default.
    // We use very low alpha (9 / 13) to mimic MASK_BRIGHTNESS 0.70 darkening.
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    let gh_u32 = u32::try_from(glass_h).expect("glass_h positive");
    // Two-pixel stagger: at MASK_BRIGHTNESS 0.70 the darker channels are ~30% down.
    // We approximate by drawing 1px dark lines on every second column with two
    // alternating tints that bias toward green/magenta when the eye averages.
    for x in (glass_x..glass_x + glass_w).step_by(2) {
        let parity = (x - glass_x) & 1;
        // Alternate tint subtly: even → magenta-ish (darken green), odd → green-ish.
        // Use low alpha black with tint; real shader does colour*mask, we do overlay.
        // Simpler: just draw a faint dark line every other pixel to break moire.
        if parity == 0 {
            canvas.set_draw_color(Color::RGBA(0, 8, 0, 9));
        } else {
            canvas.set_draw_color(Color::RGBA(8, 0, 8, 9));
        }
        let _ = canvas.fill_rect(Rect::new(x, glass_y, 1, gh_u32));
    }
    // Trinitron (type 2) would use step 3 — keep alternate path dead for now but valid.
    let _ = crt_pi::MASK_BRIGHTNESS; // ensure constant is linked / not dead
}

fn draw_vignette(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
) {
    // Barrel-aware edge vignette: thicker and darker than before so the bulge
    // edge is unmistakable. 28 px instead of 18, max ≈170 vs 108.
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    for i in 0..28 {
        let a = crate::constants::usize_mul_to_u8(i, 5).saturating_add(u8::try_from(i).unwrap_or(0));
        let a = a.min(170);
        canvas.set_draw_color(Color::RGBA(0, 0, 0, a));
        let i_i32 = usize_to_i32(i);
        let gw_u32 = u32::try_from(glass_w).expect("glass_w positive");
        let _ = canvas.fill_rect(Rect::new(glass_x, glass_y + i_i32, gw_u32, 1));
        let _ = canvas.fill_rect(Rect::new(glass_x, glass_y + glass_h - 1 - i_i32, gw_u32, 1));
    }
    for i in 0..28 {
        // Left/right vignette is barrel-inset so vertical edges bow inward at top/bottom.
        let a = crate::constants::usize_mul_to_u8(i, 5).saturating_add(u8::try_from(i).unwrap_or(0));
        let a = a.min(170);
        canvas.set_draw_color(Color::RGBA(0, 0, 0, a));
        let i_i32 = usize_to_i32(i);
        let gh_u32 = u32::try_from(glass_h).expect("glass_h positive");
        let _ = canvas.fill_rect(Rect::new(glass_x + i_i32, glass_y, 1, gh_u32));
        let _ = canvas.fill_rect(Rect::new(glass_x + glass_w - 1 - i_i32, glass_y, 1, gh_u32));
    }
    // Radial vignette overlay (soft, large) — emphasizes corner falloff and
    // barrel shape by darkening the outer 35% of the radius.
    // Drawn as sparse horizontal bands to avoid per-pixel cost.
    for y in (glass_y..glass_y + glass_h).step_by(3) {
        let yf = i32_to_f32(y);
        let gy = i32_to_f32(glass_y);
        let gh = i32_to_f32(glass_h);
        let gw = i32_to_f32(glass_w);
        let gx = i32_to_f32(glass_x);
        let vign = crt_pi::vignette_factor(
            gx + gw * 0.5,
            yf,
            gw,
            gh,
            gx,
            gy,
        );
        // vign 0.35..1.0 → alpha 0..50 at edges vs center
        let alpha_f = (1.0 - vign) * 85.0;
        if alpha_f < 2.0 {
            continue;
        }
        let a = f32_to_u8_clamped(alpha_f);
        canvas.set_draw_color(Color::RGBA(0, 0, 0, a));
        let inset = barrel_inset_for_y(y, glass_y, glass_h);
        let w = glass_w - inset * 2;
        if w > 0 {
            let _ = canvas.fill_rect(Rect::new(
                glass_x + inset,
                y,
                u32::try_from(w).expect("w positive"),
                2,
            ));
        }
    }
}

fn draw_corners(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
) {
    // Larger, darker corners so the tube's rounded inset is obvious even at
    // low magnification. Radius 34 vs previous 24, max alpha 150 vs 80.
    for r in 0..34 {
        let prod = r * 5;
        let clamped = prod.min(150);
        let a = u8::try_from(clamped).expect("0..150 fits in u8");
        canvas.set_draw_color(Color::RGBA(0, 0, 0, a));
        let r_i32 = usize_to_i32(r);
        let len = 34 - r_i32;
        let _ = canvas.draw_line(
            (glass_x + r_i32, glass_y + r_i32),
            (glass_x + r_i32 + len, glass_y + r_i32),
        );
        let _ = canvas.draw_line(
            (glass_x + glass_w - r_i32 - len, glass_y + r_i32),
            (glass_x + glass_w - r_i32, glass_y + r_i32),
        );
        let _ = canvas.draw_line(
            (glass_x + r_i32, glass_y + glass_h - r_i32),
            (glass_x + r_i32 + len, glass_y + glass_h - r_i32),
        );
        let _ = canvas.draw_line(
            (glass_x + glass_w - r_i32 - len, glass_y + glass_h - r_i32),
            (glass_x + glass_w - r_i32, glass_y + glass_h - r_i32),
        );
        let _ = canvas.draw_line(
            (glass_x + r_i32, glass_y + r_i32),
            (glass_x + r_i32, glass_y + r_i32 + len),
        );
        let _ = canvas.draw_line(
            (glass_x + glass_w - r_i32, glass_y + r_i32),
            (glass_x + glass_w - r_i32, glass_y + r_i32 + len),
        );
        let _ = canvas.draw_line(
            (glass_x + r_i32, glass_y + glass_h - r_i32 - len),
            (glass_x + r_i32, glass_y + glass_h - r_i32),
        );
        let _ = canvas.draw_line(
            (glass_x + glass_w - r_i32, glass_y + glass_h - r_i32 - len),
            (glass_x + glass_w - r_i32, glass_y + glass_h - r_i32),
        );
    }
    // Outer bezel shadow that follows the barrel — a 2px black inset around
    // the entire glass reinforces the curved edge when vignette is subtle.
    canvas.set_draw_color(Color::RGBA(0, 0, 0, 90));
    let _ = canvas.fill_rect(Rect::new(glass_x, glass_y, u32::try_from(glass_w).expect("glass_w"), 2));
    let _ = canvas.fill_rect(Rect::new(glass_x, glass_y + glass_h - 2, u32::try_from(glass_w).expect("glass_w"), 2));
    let _ = canvas.fill_rect(Rect::new(glass_x, glass_y, 2, u32::try_from(glass_h).expect("glass_h")));
    let _ = canvas.fill_rect(Rect::new(glass_x + glass_w - 2, glass_y, 2, u32::try_from(glass_h).expect("glass_h")));
}

fn draw_glass_highlights(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    flicker: f32,
) {
    canvas.set_draw_color(Color::RGBA(255, 255, 255, 10));
    let _ = canvas.fill_rect(Rect::new(
        glass_x,
        glass_y,
        u32::try_from(glass_w).expect("glass_w positive"),
        3,
    ));
    canvas.set_draw_color(Color::RGBA(255, 255, 255, 6));
    let _ = canvas.fill_rect(Rect::new(
        glass_x,
        glass_y + 3,
        u32::try_from(glass_w).expect("glass_w positive"),
        2,
    ));

    let flicker_alpha = f32_to_u8_clamped(flicker * 120.0);
    if flicker_alpha > 0 {
        canvas.set_draw_color(Color::RGBA(0, 0, 0, flicker_alpha));
        let _ = canvas.fill_rect(Rect::new(
            glass_x,
            glass_y,
            u32::try_from(glass_w).expect("glass_w positive"),
            u32::try_from(glass_h).expect("glass_h positive"),
        ));
    }
}

fn draw_bezel_glow(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
) {
    canvas.set_draw_color(Color::RGBA(0x33, 0xff, 0x66, 6));
    let _ = canvas.draw_rect(Rect::new(
        glass_x - 1,
        glass_y - 1,
        u32::try_from(glass_w + 2).expect("glass_w+2 positive"),
        u32::try_from(glass_h + 2).expect("glass_h+2 positive"),
    ));
    canvas.set_draw_color(Color::RGBA(0x33, 0xff, 0x66, 3));
    let _ = canvas.draw_rect(Rect::new(
        glass_x - 2,
        glass_y - 2,
        u32::try_from(glass_w + 4).expect("glass_w+4 positive"),
        u32::try_from(glass_h + 4).expect("glass_h+4 positive"),
    ));
}

fn draw_status_bar(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    session_active: bool,
    has_error: bool,
) {
    if has_error || !session_active {
        let bar_h: i32 = 18;
        let bar_y = glass_y + glass_h - bar_h - 4;
        canvas.set_draw_color(Color::RGBA(0x33, 0xff, 0x66, 22));
        let _ = canvas.fill_rect(Rect::new(
            glass_x + 4,
            bar_y,
            u32::try_from(glass_w - 8).expect("glass_w-8 positive"),
            u32::try_from(bar_h).expect("bar_h positive"),
        ));
    }
}
