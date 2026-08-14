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
    BEZEL_COLOR, BEZEL_HILITE, BEZEL_INNER_RADIUS, BEZEL_OUTER_RADIUS, BEZEL_SHADOW,
    GLASS_BG, GLASS_INNER_BEVEL, GLASS_RADIUS, ROWS,
};
use crate::controls::{bottom_bar_layout, ControlState, PhosphorColor};
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

// ─────────────────────────────────────────────────────────────────────────────
// Rounded-rect primitives (SDL2 has no native support)
// ─────────────────────────────────────────────────────────────────────────────

/// Fill a solid circle centred at `(cx, cy)` with `radius`.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::many_single_char_names)]
fn fill_circle(canvas: &mut Canvas<Window>, cx: i32, cy: i32, radius: i32, color: Color) {
    if radius <= 0 {
        return;
    }
    canvas.set_draw_color(color);
    let r = radius;
    let r_sq = i64::from(r) * i64::from(r);
    for dy in -r..=r {
        let dy_i64 = i64::from(dy);
        let remain = r_sq - dy_i64 * dy_i64;
        if remain < 0 {
            continue;
        }
        #[allow(clippy::cast_possible_truncation)]
        let dx = (remain as f64).sqrt() as i32;
        let y = cy + dy;
        let x0 = cx - dx;
        let w = dx * 2 + 1;
        if w > 0 {
            let _ = canvas.fill_rect(Rect::new(
                x0,
                y,
                u32::try_from(w).expect("circle w positive"),
                1,
            ));
        }
    }
}

/// Fill a rounded rectangle `x,y,w,h` with corner `radius`.
///
/// Implemented as two central rects plus four quarter-circle caps.
#[allow(clippy::many_single_char_names)]
fn fill_rounded_rect(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    color: Color,
) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = radius.clamp(0, w.min(h) / 2);
    if r <= 0 {
        canvas.set_draw_color(color);
        let _ = canvas.fill_rect(Rect::new(
            x,
            y,
            u32::try_from(w).expect("w positive"),
            u32::try_from(h).expect("h positive"),
        ));
        return;
    }
    canvas.set_draw_color(color);
    // Central horizontal band
    let _ = canvas.fill_rect(Rect::new(
        x + r,
        y,
        u32::try_from(w - 2 * r).expect("w-2r positive"),
        u32::try_from(h).expect("h positive"),
    ));
    // Central vertical band
    let _ = canvas.fill_rect(Rect::new(
        x,
        y + r,
        u32::try_from(w).expect("w positive"),
        u32::try_from(h - 2 * r).expect("h-2r positive"),
    ));
    // Four corner circles
    fill_circle(canvas, x + r, y + r, r, color);
    fill_circle(canvas, x + w - r - 1, y + r, r, color);
    fill_circle(canvas, x + r, y + h - r - 1, r, color);
    fill_circle(canvas, x + w - r - 1, y + h - r - 1, r, color);
}

/// Stroke a rounded-rect border: `border_color` ring of `thickness` surrounding
/// an inner fill of `inner_color`. The overall exterior is `x,y,w,h,radius`.
#[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
fn draw_rounded_rect_border(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    thickness: i32,
    border_color: Color,
    inner_color: Color,
) {
    if thickness <= 0 || w <= 0 || h <= 0 {
        return;
    }
    fill_rounded_rect(canvas, x, y, w, h, radius, border_color);
    let ix = x + thickness;
    let iy = y + thickness;
    let iw = w - thickness * 2;
    let ih = h - thickness * 2;
    if iw > 0 && ih > 0 {
        let ir = (radius - thickness).max(0);
        fill_rounded_rect(canvas, ix, iy, iw, ih, ir, inner_color);
    }
}

/// Stroke only the perimeter of a rounded rect (no centre fill) — used for
/// glows/shadows where we must not overpaint the glass contents.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::too_many_arguments, clippy::many_single_char_names, clippy::too_many_lines)]
fn stroke_rounded_rect(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    thickness: i32,
    color: Color,
) {
    if thickness <= 0 || w <= 0 || h <= 0 {
        return;
    }
    let r = radius.clamp(0, w.min(h) / 2);
    let t = thickness.clamp(0, 64);
    if r <= 0 {
        canvas.set_draw_color(color);
        let _ = canvas.fill_rect(Rect::new(x, y, u32::try_from(w).expect("w"), u32::try_from(t).expect("t")));
        let _ = canvas.fill_rect(Rect::new(x, y + h - t, u32::try_from(w).expect("w"), u32::try_from(t).expect("t")));
        let _ = canvas.fill_rect(Rect::new(x, y, u32::try_from(t).expect("t"), u32::try_from(h).expect("h")));
        let _ = canvas.fill_rect(Rect::new(x + w - t, y, u32::try_from(t).expect("t"), u32::try_from(h).expect("h")));
        return;
    }
    if t >= r && t * 2 >= w.min(h) {
        fill_rounded_rect(canvas, x, y, w, h, r, color);
        return;
    }
    canvas.set_draw_color(color);
    // Straight edges
    if w - 2 * r > 0 {
        let hw = u32::try_from(w - 2 * r).expect("hw");
        let ht = u32::try_from(t).expect("t");
        let _ = canvas.fill_rect(Rect::new(x + r, y, hw, ht));
        let _ = canvas.fill_rect(Rect::new(x + r, y + h - t, hw, ht));
    }
    if h - 2 * r > 0 {
        let hh = u32::try_from(h - 2 * r).expect("hh");
        let wt = u32::try_from(t).expect("t");
        let _ = canvas.fill_rect(Rect::new(x, y + r, wt, hh));
        let _ = canvas.fill_rect(Rect::new(x + w - t, y + r, wt, hh));
    }
    // Corner rings — quarter-circle bands
    let inner = r - t;
    let corners = [
        (x + r, y + r, 0),                     // top-left
        (x + w - r - 1, y + r, 1),             // top-right
        (x + r, y + h - r - 1, 2),             // bottom-left
        (x + w - r - 1, y + h - r - 1, 3),     // bottom-right
    ];
    let outer_sq = i64::from(r) * i64::from(r);
    let inner_sq = if inner > 0 { i64::from(inner) * i64::from(inner) } else { -1 };
    for (cx, cy, quad) in corners {
        for dy in -r..=r {
            let y = cy + dy;
            let dy_i64 = i64::from(dy);
            let remain_outer = outer_sq - dy_i64 * dy_i64;
            if remain_outer < 0 {
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let dx_outer = (remain_outer as f64).sqrt() as i32;
            let dx_inner = if inner > 0 && inner_sq >= dy_i64 * dy_i64 {
                let rem_inner = inner_sq - dy_i64 * dy_i64;
                #[allow(clippy::cast_possible_truncation)]
                let v = (rem_inner as f64).sqrt() as i32;
                v
            } else {
                -1
            };
            match quad {
                0 => {
                    // top-left: y <= cy, x <= cx
                    if dy > 0 { continue; }
                    let x_start = cx - dx_outer;
                    let x_end = if dx_inner >= 0 { cx - dx_inner - 1 } else { cx };
                    if x_end >= x_start {
                        let ww = x_end - x_start + 1;
                        if ww > 0 {
                            let _ = canvas.fill_rect(Rect::new(x_start, y, u32::try_from(ww).expect("ww"), 1));
                        }
                    }
                }
                1 => {
                    if dy > 0 { continue; }
                    let x_start = if dx_inner >= 0 { cx + dx_inner + 1 } else { cx };
                    let x_end = cx + dx_outer;
                    if x_end >= x_start {
                        let ww = x_end - x_start + 1;
                        let _ = canvas.fill_rect(Rect::new(x_start, y, u32::try_from(ww).expect("ww"), 1));
                    }
                }
                2 => {
                    if dy < 0 { continue; }
                    let x_start = cx - dx_outer;
                    let x_end = if dx_inner >= 0 { cx - dx_inner - 1 } else { cx };
                    if x_end >= x_start {
                        let ww = x_end - x_start + 1;
                        let _ = canvas.fill_rect(Rect::new(x_start, y, u32::try_from(ww).expect("ww"), 1));
                    }
                }
                3 => {
                    if dy < 0 { continue; }
                    let x_start = if dx_inner >= 0 { cx + dx_inner + 1 } else { cx };
                    let x_end = cx + dx_outer;
                    if x_end >= x_start {
                        let ww = x_end - x_start + 1;
                        let _ = canvas.fill_rect(Rect::new(x_start, y, u32::try_from(ww).expect("ww"), 1));
                    }
                }
                _ => {}
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bezel — curvy CRT body
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub fn draw_bezel(canvas: &mut Canvas<Window>) {
    let ww = window_w_i32();
    let wh = window_h_i32();

    // Desk / wall behind the monitor — very dark so rounded outer corners read
    // as the monitor's silhouette instead of the window's square.
    canvas.set_draw_color(Color::RGB(0x0a, 0x0a, 0x0c));
    canvas.clear();
    canvas.set_blend_mode(sdl2::render::BlendMode::None);

    // Outer monitor body — big rounded rect, not a sharp box.
    fill_rounded_rect(canvas, 0, 0, ww, wh, BEZEL_OUTER_RADIUS, BEZEL_COLOR);

    // Outer highlight that follows the rounded silhouette (curved plastic).
    // A thin hilite ring on the exterior makes the bulge obvious.
    draw_rounded_rect_border(
        canvas,
        0,
        0,
        ww,
        wh,
        BEZEL_OUTER_RADIUS,
        4,
        BEZEL_HILITE,
        BEZEL_COLOR,
    );

    // Subtle convex shading — lighter at centre to suggest the plastic itself
    // is bowed outward (thicker at corners, thinner at mid-edge). We use a
    // single very low-alpha rounded wash; no extra bottom-half darkening so
    // top and bottom bezels remain visually identical (no double border).
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    // Centre bulge highlight (very soft)
    {
        let cx = ww / 2;
        let cy = wh / 2;
        let bw = ww - 120;
        let bh = wh - 120;
        let bx = cx - bw / 2;
        let by = cy - bh / 2;
        canvas.set_draw_color(Color::RGBA(0xff, 0xff, 0xff, 7));
        fill_rounded_rect(canvas, bx, by, bw, bh, BEZEL_INNER_RADIUS, Color::RGBA(0xff, 0xff, 0xff, 7));
        // alpha 7 is barely visible — reads as a gentle convex sheen without
        // creating a distinct band.
    }
    canvas.set_blend_mode(sdl2::render::BlendMode::None);

    // Inner recessed lip around the tube — a darker rounded ring that frames
    // the glass and creates the classic CRT "tube set back from plastic" look.
    // The lip itself is rounded; its corners are thicker than its mid-edges
    // (because the outer radius is larger than glass radius).
    {
        let lip = 8;
        let lx = BEZEL - lip;
        let ly = BEZEL - lip;
        let lw = ww - (BEZEL - lip) * 2;
        let lh = wh - (BEZEL - lip) * 2;
        let lr = GLASS_RADIUS + lip;
        // Dark lip — uses BEZEL_SHADOW for consistent palette
        draw_rounded_rect_border(
            canvas,
            lx,
            ly,
            lw,
            lh,
            lr,
            lip,
            BEZEL_SHADOW,
            BEZEL_COLOR,
        );
        // Top inner bevel highlight on the lip (curved, follows radius)
        canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
        canvas.set_draw_color(Color::RGBA(0x6a, 0x6a, 0x62, 42));
        // Two-pixel hilite ring just inside the lip, top half only approximated
        // by drawing a full thin ring and then covering the bottom half
        let hr = lr - 1;
        let hx = lx + 1;
        let hy = ly + 1;
        let hw = lw - 2;
        let hh = lh - 2;
        draw_rounded_rect_border(
            canvas,
            hx,
            hy,
            hw,
            hh,
            hr,
            2,
            Color::RGBA(0x6a, 0x6a, 0x62, 42),
            BEZEL_COLOR,
        );
        // Cover bottom half of that hilite with bezel so only top edge glints
        canvas.set_draw_color(BEZEL_COLOR);
        let mid_y = ly + lh / 2;
        let _ = canvas.fill_rect(Rect::new(
            hx,
            mid_y,
            u32::try_from(hw).expect("hw positive"),
            u32::try_from(lh / 2).expect("half positive"),
        ));
        // Re-establish the glass hole (glass will paint over, but keep bezel clean)
        fill_rounded_rect(
            canvas,
            BEZEL,
            BEZEL,
            ww - BEZEL * 2,
            wh - BEZEL * 2,
            GLASS_RADIUS,
            BEZEL_COLOR,
        );
        // Inner shadow on lip bottom edge (subtle)
        canvas.set_draw_color(Color::RGBA(0x00, 0x00, 0x00, 34));
        draw_rounded_rect_border(
            canvas,
            lx + 2,
            ly + 2,
            lw - 4,
            lh - 4,
            lr - 2,
            2,
            Color::RGBA(0x00, 0x00, 0x00, 34),
            BEZEL_COLOR,
        );
        fill_rounded_rect(
            canvas,
            BEZEL,
            BEZEL,
            ww - BEZEL * 2,
            wh - BEZEL * 2,
            GLASS_RADIUS,
            BEZEL_COLOR,
        );
        canvas.set_blend_mode(sdl2::render::BlendMode::None);
    }

}

/// Bottom bezel control strip — 3-position phosphor switch + 3 toggles.
/// Draws on the 48 px bezel bar below the glass. LEDs kept far right.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::trivially_copy_pass_by_ref,
    clippy::many_single_char_names,
    clippy::if_same_then_else
)]
pub fn draw_bottom_controls(
    canvas: &mut Canvas<Window>,
    state: &ControlState,
    hover: Option<(i32, i32)>,
) {
    let layout = bottom_bar_layout();
    let hover_hit = hover.and_then(|(x, y)| crate::controls::hit_test(x, y));

    // Label helper — draw text via texture if a font is available.
    // Since this function is called every frame, we keep labels minimal and
    // fall back to coloured indicators when font rendering is expensive.
    // For now we draw small caption rects + rely on colour coding.
    // Actual text captions are drawn by `draw_bottom_control_labels` if a font is supplied.

    // ── Phosphor switch ──
    {
        let track = layout.phosphor.track;
        // Track background — dark rounded rect
        fill_rounded_rect(
            canvas,
            track.x(),
            track.y(),
            track.width() as i32,
            track.height() as i32,
            6,
            Color::RGB(0x1a, 0x1a, 0x18),
        );
        // Segments
        for (idx, seg) in layout.phosphor.segments.iter().enumerate() {
            let pc = PhosphorColor::from_index(idx);
            let is_selected = state.phosphor == pc;
            let is_hovered = hover_hit == Some(crate::controls::ControlHit::Phosphor(pc));
            let base = match pc {
                PhosphorColor::Green => Color::RGB(0x33, 0xFF, 0x66),
                PhosphorColor::Amber => Color::RGB(0xFF, 0xB0, 0x00),
                PhosphorColor::White => Color::RGB(0xE0, 0xE0, 0xC0),
            };
            let fill = if is_selected {
                if is_hovered {
                    // slightly brighter on hover
                    match pc {
                        PhosphorColor::Green => Color::RGB(0x55, 0xFF, 0x88),
                        PhosphorColor::Amber => Color::RGB(0xFF, 0xC8, 0x33),
                        PhosphorColor::White => Color::RGB(0xFF, 0xFF, 0xE8),
                    }
                } else {
                    base
                }
            } else if is_hovered {
                Color::RGB(0x4a, 0x4a, 0x44)
            } else {
                Color::RGB(0x2a, 0x2a, 0x26)
            };
            // Inset 1px so track border shows
            let inset = if is_selected { 1 } else { 2 };
            let rx = seg.x() + inset;
            let ry = seg.y() + inset;
            let rw = seg.width() as i32 - inset * 2;
            let rh = seg.height() as i32 - inset * 2;
            if rw > 0 && rh > 0 {
                let r = if idx == 0 || idx == 2 { 5 } else { 2 };
                // For end caps use larger radius, middle tighter
                let radius = if is_selected { r } else { 3 };
                fill_rounded_rect(canvas, rx, ry, rw, rh, radius, fill);
            }
            // Selected indicator dot
            if is_selected {
                canvas.set_draw_color(Color::RGBA(0x00, 0x00, 0x00, 160));
                let cx = seg.x() + seg.width() as i32 / 2;
                let cy = seg.y() + seg.height() as i32 / 2;
                fill_circle(canvas, cx, cy, 3, Color::RGBA(0x00, 0x00, 0x00, 120));
                fill_circle(canvas, cx, cy, 2, Color::RGB(0x00, 0x00, 0x00));
                // white centre
                fill_circle(canvas, cx, cy, 1, Color::RGB(0xFF, 0xFF, 0xFF));
            }
        }
        // Outer border
        canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
        stroke_rounded_rect(
            canvas,
            track.x(),
            track.y(),
            track.width() as i32,
            track.height() as i32,
            6,
            1,
            Color::RGBA(0x6a, 0x6a, 0x62, 60),
        );
        canvas.set_blend_mode(sdl2::render::BlendMode::None);
    }

    // ── Toggle helper ──
    let draw_toggle = |canvas: &mut Canvas<Window>, layout_rect: Rect, enabled: bool, hit: bool| {
        let on_track = Color::RGB(0x2a, 0x6a, 0x3a);
        let on_track_hover = Color::RGB(0x33, 0x88, 0x4a);
        let off_track = Color::RGB(0x2e, 0x2e, 0x2a);
        let off_track_hover = Color::RGB(0x3a, 0x3a, 0x36);
        let track_color = if enabled {
            if hit {
                on_track_hover
            } else {
                on_track
            }
        } else if hit {
            off_track_hover
        } else {
            off_track
        };
        fill_rounded_rect(
            canvas,
            layout_rect.x(),
            layout_rect.y(),
            layout_rect.width() as i32,
            layout_rect.height() as i32,
            9,
            track_color,
        );
        // Border
        canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
        stroke_rounded_rect(
            canvas,
            layout_rect.x(),
            layout_rect.y(),
            layout_rect.width() as i32,
            layout_rect.height() as i32,
            9,
            1,
            Color::RGBA(0x6a, 0x6a, 0x62, 42),
        );
        canvas.set_blend_mode(sdl2::render::BlendMode::None);
        // Knob
        let knob_r: i32 = 7;
        let cy = layout_rect.y() + layout_rect.height() as i32 / 2;
        let cx = if enabled {
            layout_rect.x() + layout_rect.width() as i32 - 9
        } else {
            layout_rect.x() + 9
        };
        let knob_col = if enabled {
            Color::RGB(0x88, 0xFF, 0xAA)
        } else {
            Color::RGB(0x9a, 0x9a, 0x96)
        };
        // knob shadow
        fill_circle(canvas, cx + 1, cy + 1, knob_r, Color::RGBA(0x00, 0x00, 0x00, 60));
        fill_circle(canvas, cx, cy, knob_r, knob_col);
        fill_circle(canvas, cx, cy, knob_r - 2, Color::RGB(0xFF, 0xFF, 0xFF));
        // inner dot when on
        if enabled {
            fill_circle(canvas, cx, cy, 2, knob_col);
        }
    };

    let curv_hover = hover_hit == Some(crate::controls::ControlHit::Curvature);
    let flicker_hover = hover_hit == Some(crate::controls::ControlHit::Flicker);
    let scan_hover = hover_hit == Some(crate::controls::ControlHit::Scanlines);

    draw_toggle(
        canvas,
        layout.curvature.track,
        state.curvature_enabled,
        curv_hover,
    );
    draw_toggle(
        canvas,
        layout.flicker.track,
        state.flicker_enabled,
        flicker_hover,
    );
    draw_toggle(
        canvas,
        layout.scanlines.track,
        state.scanlines_enabled,
        scan_hover,
    );

    // ── Captions (primitive) ──
    // Small 1-px high caption bars above each control, colour-coded, with a
    // tiny pixel-font label drawn via filled rects. Keeps the bar readable
    // without needing a second font load. Y is chosen to sit in the gap
    // between the lip (8px below glass) and the control tracks, with
    // symmetric padding so bottom bezel looks evenly centred.
    let caption_y = layout.center_y - 14;
    // Phosphor label: three tiny dots showing G/A/W colours + "PHOSPHOR" hint via bar
    {
        let cx = layout.phosphor.track.x() + layout.phosphor.track.width() as i32 / 2;
        // 3-dot legend — centred just below the lip, above the track
        let dot_y = caption_y;
        for (idx, pc) in [PhosphorColor::Green, PhosphorColor::Amber, PhosphorColor::White]
            .iter()
            .enumerate()
        {
            let dx = cx - 14 + (idx as i32) * 14;
            let is_sel = *pc == state.phosphor;
            let col = pc.color();
            let dim_col = Color::RGB(col.r / 3, col.g / 3, col.b / 3);
            let c = if is_sel { col } else { dim_col };
            fill_circle(canvas, dx, dot_y, 3, c);
            if is_sel {
                stroke_rounded_rect(
                    canvas,
                    dx - 4,
                    dot_y - 4,
                    8,
                    8,
                    4,
                    1,
                    Color::RGBA(0xFF, 0xFF, 0xFF, 90),
                );
            }
        }
    }
    // Toggle captions — single letters C / F / S with on/off brightness
    let draw_caption = |canvas: &mut Canvas<Window>, x: i32, y: i32, enabled: bool| {
        let col = if enabled {
            Color::RGB(0x88, 0xFF, 0xAA)
        } else {
            Color::RGB(0x6a, 0x6a, 0x62)
        };
        // 8×2 bar
        let _ = canvas.fill_rect(Rect::new(x - 4, y, 8, 2));
        canvas.set_draw_color(col);
        let _ = canvas.fill_rect(Rect::new(x - 4, y, 8, 2));
    };
    canvas.set_draw_color(Color::RGB(0x88, 0xFF, 0xAA));
    draw_caption(
        canvas,
        layout.curvature.track.x() + 18,
        caption_y,
        state.curvature_enabled,
    );
    draw_caption(
        canvas,
        layout.flicker.track.x() + 18,
        caption_y,
        state.flicker_enabled,
    );
    draw_caption(
        canvas,
        layout.scanlines.track.x() + 18,
        caption_y,
        state.scanlines_enabled,
    );
    // Tiny text fallback: we use the bezel hilite to stamp a 1-pixel high line
    // that is visible as a separator; full textual labels are drawn by the
    // textured variant below when a font is supplied.
    let _ = caption_y;
}

/// Variant of `draw_bottom_controls` that also stamps textual labels using a font.
/// Call this after `draw_bottom_controls` if you want crisp TTF captions.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::many_single_char_names,
    clippy::trivially_copy_pass_by_ref,
    clippy::manual_let_else
)]
pub fn draw_bottom_control_labels(
    canvas: &mut Canvas<Window>,
    font: &sdl2::ttf::Font<'_, '_>,
    state: &ControlState,
) {
    let layout = bottom_bar_layout();
    let captions = [
        (layout.phosphor.track.x() + layout.phosphor.track.width() as i32 / 2, "PHOSPHOR"),
        (layout.curvature.track.x() + layout.curvature.track.width() as i32 / 2, "CURVE"),
        (layout.flicker.track.x() + layout.flicker.track.width() as i32 / 2, "FLICKER"),
        (layout.scanlines.track.x() + layout.scanlines.track.width() as i32 / 2, "SCAN"),
    ];
    for (cx, text) in captions {
        let surf = match font.render(text).blended(Color::RGBA(0xcc, 0xcc, 0xc0, 180)) {
            Ok(s) => s,
            Err(_) => continue,
        };
        // Render at a tiny size — scale down via texture size
        let creator = canvas.texture_creator();
        let tex = match creator.create_texture_from_surface(&surf) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let q = tex.query();
        // Clamp to small caption: max 48px wide, 10px tall
        let w = (q.width as i32).min(52);
        let h = (q.height as i32).min(10);
        let x = cx - w / 2;
        // place just above each track with 6px gap; keep inside bezel
        // track tops are aligned, so use the phosphor track as reference for Y
        // and compute per-caption to stay above its own track.
        let track_top = layout.phosphor.track.y();
        let mut y = track_top - h - 6;
        // ensure we never paint inside the glass (above bar_y) and leave padding
        let min_y = layout.bar_y + 2;
        if y < min_y {
            y = min_y;
        }
        // per-caption adjustment: curve/flicker/scan tracks are at same Y,
        // so y computed once is valid for all.
        let _ = (cx, text);
        let dst = Rect::new(x, y, w as u32, h as u32);
        let _ = canvas.copy(&tex, None, dst);
    }
    // Selected phosphor value under the switch — keep with padding from bottom edge
    {
        let text = state.phosphor.label();
        let col = state.phosphor.color();
        // dim white for readability on dark bezel: use phosphor tint with alpha
        let label_col = Color::RGB(col.r, col.g, col.b);
        let surf = match font.render(text).blended(label_col) {
            Ok(s) => s,
            Err(_) => return,
        };
        let creator = canvas.texture_creator();
        let mut tex = match creator.create_texture_from_surface(&surf) {
            Ok(t) => t,
            Err(_) => return,
        };
        tex.set_blend_mode(sdl2::render::BlendMode::Blend);
        tex.set_alpha_mod(200);
        let q = tex.query();
        let w = (q.width as i32).min(60);
        let h = (q.height as i32).min(10);
        let cx = layout.phosphor.track.x() + layout.phosphor.track.width() as i32 / 2;
        let x = cx - w / 2;
        let mut y = layout.phosphor.track.y() + layout.phosphor.track.height() as i32 + 4;
        // ensure at least 4px padding from the outer window edge (rounded corner)
        // and avoid overlapping the track itself
        let max_y = window_h_i32() - h - 4;
        if y > max_y {
            y = max_y;
        }
        let dst = Rect::new(x, y, w as u32, h as u32);
        let _ = canvas.copy(&tex, None, dst);
    }
}

pub fn draw_power_led(canvas: &mut Canvas<Window>, session_active: bool) {
    // Vertically centred in the bottom bezel (same centre as the toggle tracks)
    // and inset from the outer rounded corner so it is never clipped by the
    // 28px outer radius.
    let layout = crate::controls::bottom_bar_layout();
    let led_x = window_w_i32() - BEZEL - 28;
    let led_y = layout.center_y - 5;
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
    // Main tube face — rounded, slightly convex. The 80×24 text grid stays
    // rectangular inside for readability; only the glass frame/bezel is curvy.
    fill_rounded_rect(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        GLASS_RADIUS,
        GLASS_BG,
    );
    // Deep inner frame (tube edge) — 3px dark ring following the rounded shape
    // Use neutral dark (no phosphor tint) so the bottom edge reads as shadow,
    // not a green line. Previously 0x050a06 / 0x142a18 leaked green at the
    // glass/bezel interface.
    draw_rounded_rect_border(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        GLASS_RADIUS,
        GLASS_INNER_BEVEL,
        Color::RGB(0x0a, 0x0a, 0x0a),
        GLASS_BG,
    );
    // Second bevel ring — slightly lighter neutral, to create a stepped glass
    // edge without phosphor tint. Bottom edge stays dark like the top inner
    // shadow instead of showing a green wash.
    draw_rounded_rect_border(
        canvas,
        glass_x + 2,
        glass_y + 2,
        glass_w - 4,
        glass_h - 4,
        GLASS_RADIUS - 2,
        2,
        Color::RGB(0x1e, 0x1e, 0x1e),
        GLASS_BG,
    );
    // Subtle convex highlight on the glass itself — a soft horizontal sheen
    // near the top third that follows the rounded top edge.
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    canvas.set_draw_color(Color::RGBA(0xff, 0xff, 0xff, 9));
    // Horizontal highlight band: inset so its ends are rounded and don't poke
    // out of the glass corners.
    let hx = glass_x + GLASS_RADIUS;
    let hw = glass_w - GLASS_RADIUS * 2;
    if hw > 0 {
        let _ = canvas.fill_rect(Rect::new(
            hx,
            glass_y + 10,
            u32::try_from(hw).expect("hw positive"),
            10,
        ));
        let _ = canvas.fill_rect(Rect::new(
            hx,
            glass_y + 22,
            u32::try_from(hw).expect("hw positive"),
            3,
        ));
    }
    // Corner-round the highlight ends with small white caps so the sheen
    // doesn't look square-cut against the rounded glass.
    canvas.set_draw_color(Color::RGBA(0xff, 0xff, 0xff, 7));
    fill_circle(canvas, glass_x + GLASS_RADIUS, glass_y + 15, 6, Color::RGBA(0xff, 0xff, 0xff, 7));
    fill_circle(
        canvas,
        glass_x + glass_w - GLASS_RADIUS - 1,
        glass_y + 15,
        6,
        Color::RGBA(0xff, 0xff, 0xff, 7),
    );
    canvas.set_blend_mode(sdl2::render::BlendMode::None);
    let _ = metrics;
    (glass_x, glass_y, glass_w, glass_h)
}

/// Compute a barrel inset for a given `y` row inside the glass.
/// Used to bow scanlines and inset the text so the screen looks bulged
/// (narrower at top/bottom, widest at midline). Takes explicit `params`
/// so runtime toggles can zero curvature.
#[allow(dead_code)]
fn barrel_inset_for_y(y: i32, glass_y: i32, glass_h: i32) -> i32 {
    barrel_inset_for_y_with_params(y, glass_y, glass_h, &crt_pi::CrtPiParams::default())
}

fn barrel_inset_for_y_with_params(
    y: i32,
    glass_y: i32,
    glass_h: i32,
    params: &crt_pi::CrtPiParams,
) -> i32 {
    let gf = i32_to_f32(glass_h);
    let yf = i32_to_f32(y);
    let gy = i32_to_f32(glass_y);
    if gf <= 0.0 {
        return 0;
    }
    if params.curvature_x == 0.0 {
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
#[allow(dead_code, clippy::too_many_arguments)]
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

/// Control-aware wrapper — uses `ControlState` to pick phosphor colours
/// and curvature. `flicker`/`hum` already respect `state.flicker_enabled`.
#[allow(clippy::too_many_arguments, clippy::trivially_copy_pass_by_ref)]
pub fn draw_grid_text_with_controls(
    canvas: &mut Canvas<Window>,
    grid: &Grid,
    font: &sdl2::ttf::Font<'_, '_>,
    metrics: &GridMetrics,
    flicker: f32,
    hum: f32,
    blink_on: bool,
    session_active: bool,
    state: &ControlState,
) -> Result<(), String> {
    let flicker_eff = if state.flicker_enabled { flicker } else { 0.0 };
    let hum_eff = if state.flicker_enabled { hum } else { 0.0 };
    draw_grid_text_with_params_and_phosphor(
        canvas,
        grid,
        font,
        metrics,
        flicker_eff,
        hum_eff,
        blink_on,
        session_active,
        state.crt_params(),
        state,
    )
}

/// Variant that accepts explicit `CrtPiParams` (curvature override via `--curvature`).
#[allow(dead_code, clippy::too_many_arguments)]
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
    // Default phosphor = Green for callers that don't specify ControlState.
    let default_state = ControlState {
        phosphor: PhosphorColor::Green,
        curvature_enabled: params.curvature_x != 0.0 || params.curvature_y != 0.0,
        flicker_enabled: true,
        scanlines_enabled: true,
    };
    draw_grid_text_with_params_and_phosphor(
        canvas,
        grid,
        font,
        metrics,
        flicker,
        hum,
        blink_on,
        session_active,
        params,
        &default_state,
    )
}

#[allow(clippy::too_many_arguments, clippy::trivially_copy_pass_by_ref)]
pub fn draw_grid_text_with_params_and_phosphor(
    canvas: &mut Canvas<Window>,
    grid: &Grid,
    font: &sdl2::ttf::Font<'_, '_>,
    metrics: &GridMetrics,
    flicker: f32,
    hum: f32,
    blink_on: bool,
    session_active: bool,
    params: crt_pi::CrtPiParams,
    state: &ControlState,
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
        // When curvature is disabled, skip distort entirely.
        let warped_origin_x = if state.curvature_enabled {
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
                let inset = barrel_inset_for_y_with_params(y_px, glass_y, glass_h, &params);
                (-i32_to_f32(inset) * 0.5, 0.0)
            };
            #[allow(clippy::cast_possible_truncation)]
            {
                metrics.origin_x - (dx / 1.4).clamp(-10.0, 10.0) as i32
            }
        } else {
            metrics.origin_x
        };

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
                state,
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
                state,
            );
        }
    }
    Ok(())
}

// Rendering needs canvas, font, position and CRT flicker; grouping would obscure intent.
// Allow too_many_arguments with justification: bloom rendering is inherently multi-param.
#[allow(clippy::too_many_arguments, clippy::trivially_copy_pass_by_ref)]
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
    state: &ControlState,
) -> Result<(), String> {
    // Bloom halo — matches crt-pi BLOOM_FACTOR intent: bright scanlines widen.
    // SDL cannot do per-pixel bloom, so we approximate with two offset blits in
    // phosphor bloom at reduced alpha.
    let bloom_color = state.phosphor_bloom();
    let phosphor_color = state.phosphor_color();
    let bloom_surf = font
        .render(line)
        .blended(bloom_color)
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
        .blended(phosphor_color)
        .map_err(|e| e.to_string())?;
    let mut tex = texture_creator
        .create_texture_from_surface(&surf)
        .map_err(|e| e.to_string())?;
    tex.set_blend_mode(sdl2::render::BlendMode::Blend);
    let alpha_f = if state.flicker_enabled {
        255.0 * (1.0 - flicker * 0.5 - hum * 0.3)
    } else {
        255.0
    };
    let mut alpha = f32_to_u8_clamped(alpha_f);
    if state.flicker_enabled && alpha < 200 {
        alpha = 200;
    }
    tex.set_alpha_mod(alpha);
    let dst = Rect::new(origin_x, y_px, width, height);
    let _ = canvas.copy(&tex, None, dst);
    Ok(())
}

// Cursor needs geometry + session state; 8 args is domain-justified for single-rect draw.
#[allow(clippy::too_many_arguments, clippy::trivially_copy_pass_by_ref)]
fn draw_cursor(
    canvas: &mut Canvas<Window>,
    origin_x: i32,
    y_px: i32,
    cursor_x: usize,
    cell_w_i32: i32,
    cell_w: u32,
    cell_h: u32,
    session_active: bool,
    state: &ControlState,
) {
    let cx = origin_x + usize_to_i32(cursor_x) * cell_w_i32;
    let cy = y_px;
    let cursor_color = if session_active {
        state.phosphor_color()
    } else {
        state.phosphor_dim()
    };
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    canvas.set_draw_color(cursor_color);
    let _ = canvas.fill_rect(Rect::new(cx, cy, cell_w, cell_h));
    // Inner highlight — tinted toward phosphor but keep light centre for visibility
    let ph = state.phosphor_color();
    let inner = Color::RGB(
        ph.r.saturating_add(0x88),
        ph.g.saturating_add(0x88),
        ph.b.saturating_add(0x55),
    );
    canvas.set_draw_color(inner);
    let _ = canvas.fill_rect(Rect::new(cx + 1, cy + 1, cell_w - 2, cell_h - 2));
}

// Decomposed CRT effects to keep each function <100 lines and avoid too_many_lines

#[allow(dead_code, clippy::too_many_arguments)]
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
    // Legacy wrapper — all effects enabled, green phosphor.
    let default_state = ControlState::default();
    draw_scanlines_and_vignette_with_state(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        t,
        flicker,
        session_active,
        has_error,
        &default_state,
    );
}

#[allow(clippy::too_many_arguments, clippy::trivially_copy_pass_by_ref)]
pub fn draw_scanlines_and_vignette_with_state(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    t: f32,
    flicker: f32,
    session_active: bool,
    has_error: bool,
    state: &ControlState,
) {
    let flicker_eff = if state.flicker_enabled { flicker } else { 0.0 };
    if state.scanlines_enabled {
        draw_scanlines_with_state(canvas, glass_x, glass_y, glass_w, glass_h, t, state);
        draw_aperture_mask(canvas, glass_x, glass_y, glass_w, glass_h);
    }
    draw_vignette_with_state(canvas, glass_x, glass_y, glass_w, glass_h, state);
    draw_corners(canvas, glass_x, glass_y, glass_w, glass_h);
    draw_glass_highlights(canvas, glass_x, glass_y, glass_w, glass_h, flicker_eff);
    draw_bezel_glow_with_state(canvas, glass_x, glass_y, glass_w, glass_h, state);
    draw_status_bar_with_state(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        session_active,
        has_error,
        state,
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
#[allow(dead_code)]
fn draw_scanlines(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    t: f32,
) {
    let default_state = ControlState::default();
    draw_scanlines_with_state(canvas, glass_x, glass_y, glass_w, glass_h, t, &default_state);
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn draw_scanlines_with_state(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    t: f32,
    state: &ControlState,
) {
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    let params = state.crt_params();
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
        let inset = if state.curvature_enabled {
            barrel_inset_for_y_with_params(y, glass_y, glass_h, &params)
        } else {
            0
        };
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

#[allow(dead_code)]
fn draw_vignette(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
) {
    draw_vignette_with_state(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        &ControlState::default(),
    );
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn draw_vignette_with_state(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    state: &ControlState,
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
    // Radial vignette overlay — only when curvature is on (it emphasizes barrel).
    if !state.curvature_enabled {
        return;
    }
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
        let params = state.crt_params();
        let inset = barrel_inset_for_y_with_params(y, glass_y, glass_h, &params);
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
    // Adapted to rounded glass: we darken a circular falloff rather than
    // just square corners so the rounded glass reads correctly.
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
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
    // Rounded glass edge shadow — a thin dark rounded border that reinforces
    // the curved tube edge when vignette is subtle. Drawn as a rounded ring.
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    stroke_rounded_rect(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        GLASS_RADIUS,
        2,
        Color::RGBA(0, 0, 0, 85),
    );
    // Fallback square edge for compatibility (overpainted by rounded where visible)
    canvas.set_draw_color(Color::RGBA(0, 0, 0, 40));
    let _ = canvas.fill_rect(Rect::new(glass_x, glass_y, u32::try_from(glass_w).expect("glass_w"), 2));
    let _ = canvas.fill_rect(Rect::new(glass_x, glass_y + glass_h - 2, u32::try_from(glass_w).expect("glass_w"), 2));
}

fn draw_glass_highlights(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    flicker: f32,
) {
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    // Top glass reflection — inset by radius so it hugs the rounded top edge
    canvas.set_draw_color(Color::RGBA(255, 255, 255, 10));
    let hx = glass_x + GLASS_RADIUS;
    let hw = glass_w - GLASS_RADIUS * 2;
    if hw > 0 {
        let _ = canvas.fill_rect(Rect::new(
            hx,
            glass_y,
            u32::try_from(hw).expect("hw positive"),
            3,
        ));
        canvas.set_draw_color(Color::RGBA(255, 255, 255, 6));
        let _ = canvas.fill_rect(Rect::new(
            hx,
            glass_y + 3,
            u32::try_from(hw).expect("hw positive"),
            2,
        ));
    }
    // Corner caps for the highlight so the ends are rounded, not square
    canvas.set_draw_color(Color::RGBA(255, 255, 255, 9));
    fill_circle(canvas, glass_x + GLASS_RADIUS, glass_y + 2, 3, Color::RGBA(255, 255, 255, 9));
    fill_circle(
        canvas,
        glass_x + glass_w - GLASS_RADIUS - 1,
        glass_y + 2,
        3,
        Color::RGBA(255, 255, 255, 9),
    );

    let flicker_alpha = f32_to_u8_clamped(flicker * 120.0);
    if flicker_alpha > 0 {
        canvas.set_draw_color(Color::RGBA(0, 0, 0, flicker_alpha));
        // Flicker overlay should be rounded to match glass shape
        fill_rounded_rect(
            canvas,
            glass_x,
            glass_y,
            glass_w,
            glass_h,
            GLASS_RADIUS,
            Color::RGBA(0, 0, 0, flicker_alpha),
        );
    }
}

fn draw_bezel_glow(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
) {
    // Neutral dark halo — previously phosphor-tinted (0x33ff66) which produced a
    // visible green line at the glass bottom just above the bezel controls.
    // Use dark neutral so the edge reads as shadow, not phosphor bleed.
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    stroke_rounded_rect(
        canvas,
        glass_x - 1,
        glass_y - 1,
        glass_w + 2,
        glass_h + 2,
        GLASS_RADIUS + 1,
        1,
        Color::RGBA(0x00, 0x00, 0x00, 14),
    );
    stroke_rounded_rect(
        canvas,
        glass_x - 2,
        glass_y - 2,
        glass_w + 4,
        glass_h + 4,
        GLASS_RADIUS + 2,
        1,
        Color::RGBA(0x00, 0x00, 0x00, 8),
    );
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn draw_bezel_glow_with_state(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    _state: &ControlState,
) {
    // Neutral halo only — no phosphor-tinted spill so the bottom edge stays
    // dark for every phosphor colour (green/amber/white).
    draw_bezel_glow(canvas, glass_x, glass_y, glass_w, glass_h);
}

#[allow(dead_code)]
fn draw_status_bar(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    session_active: bool,
    has_error: bool,
) {
    draw_status_bar_with_state(
        canvas,
        glass_x,
        glass_y,
        glass_w,
        glass_h,
        session_active,
        has_error,
        &ControlState::default(),
    );
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_arguments)]
fn draw_status_bar_with_state(
    canvas: &mut Canvas<Window>,
    glass_x: i32,
    glass_y: i32,
    glass_w: i32,
    glass_h: i32,
    session_active: bool,
    has_error: bool,
    state: &ControlState,
) {
    if has_error || !session_active {
        let bar_h: i32 = 18;
        let bar_y = glass_y + glass_h - bar_h - 4;
        canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
        let ph = state.phosphor_color();
        canvas.set_draw_color(Color::RGBA(ph.r, ph.g, ph.b, 22));
        // Rounded status bar that sits inside the rounded glass
        let bx = glass_x + 4;
        let bw = glass_w - 8;
        fill_rounded_rect(
            canvas,
            bx,
            bar_y,
            bw,
            bar_h,
            6,
            Color::RGBA(ph.r, ph.g, ph.b, 22),
        );
    }
}
