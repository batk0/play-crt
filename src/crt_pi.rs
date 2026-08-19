//! `crt_pi` — Rust CPU implementation of CRT effects for the SDL2 fallback.
//!
//! The GL path now uses `assets/shaders/crt-lottes.vert` / `.frag`
//! (PUBLIC DOMAIN CRT STYLED SCAN-LINE SHADER by Timothy Lottes, vendored
//! as `crt-lottes.glsl`). This file provides similar effects in pure Rust
//! for the SDL2 CPU renderer so the fallback has no GL dependency.
//!
//! Covered effects:
//! - gamma-aware scanline weighting (scanline weight + multisample)
//! - bloom factor, aperture mask (green/magenta, Trinitron)
//! - barrel distortion for curvature
//! - `filterWidth = InputSize.y / OutputSize.y / 3` multisample
//!
//! This CPU module is MIT (© 2026 batk0, clean-room). The GL shader is
//! PUBLIC DOMAIN (lottes). They share similar tunable defaults (see
//! `CrtPiParams`) but are now separate paths — no copyleft-licensed code remains.

#![allow(clippy::pedantic)]

use std::sync::OnceLock;

/// Optional global override for `CrtPiParams::default()` (set via `--curvature`).
static CURVATURE_OVERRIDE: OnceLock<std::sync::Mutex<Option<CrtPiParams>>> = OnceLock::new();

fn override_slot() -> &'static std::sync::Mutex<Option<CrtPiParams>> {
    CURVATURE_OVERRIDE.get_or_init(|| std::sync::Mutex::new(None))
}

/// Set the global curvature override used by `CrtPiParams::default()`.
/// Called once at startup from `main` when `--curvature` is supplied.
pub fn set_curvature_override(params: CrtPiParams) {
    if let Ok(mut g) = override_slot().lock() {
        *g = Some(params);
    }
}

/// Returns the current override if set.
#[must_use]
pub fn curvature_override() -> Option<CrtPiParams> {
    override_slot().lock().ok().and_then(|g| *g)
}

/// Default CRT parameters — tuned for obviously visible barrel bulge
/// on `80×24` text. Early defaults of `0.10`/`0.25` were too subtle at
/// this resolution; we use `0.20`/`0.20` so curvature is unmistakable
/// while staying readable. Override via `CrtPiParams` or `--curvature`.
pub const CURVATURE_X: f32 = 0.20;
pub const CURVATURE_Y: f32 = 0.20;
#[allow(dead_code)]
pub const MASK_BRIGHTNESS: f32 = 0.70;
pub const SCANLINE_WEIGHT: f32 = 6.0;
pub const SCANLINE_GAP_BRIGHTNESS: f32 = 0.12;
pub const BLOOM_FACTOR: f32 = 1.5;
pub const INPUT_GAMMA: f32 = 2.4;
pub const OUTPUT_GAMMA: f32 = 2.2;

/// Tunable parameter bundle (mirrors GLSL uniforms).
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct CrtPiParams {
    pub curvature_x: f32,
    pub curvature_y: f32,
    pub mask_brightness: f32,
    pub scanline_weight: f32,
    pub scanline_gap: f32,
    pub bloom_factor: f32,
    pub input_gamma: f32,
    pub output_gamma: f32,
}

impl Default for CrtPiParams {
    fn default() -> Self {
        if let Some(ov) = curvature_override() {
            return ov;
        }
        Self {
            curvature_x: CURVATURE_X,
            curvature_y: CURVATURE_Y,
            mask_brightness: MASK_BRIGHTNESS,
            scanline_weight: SCANLINE_WEIGHT,
            scanline_gap: SCANLINE_GAP_BRIGHTNESS,
            bloom_factor: BLOOM_FACTOR,
            input_gamma: INPUT_GAMMA,
            output_gamma: OUTPUT_GAMMA,
        }
    }
}

/// Scanline weight core: `max(1 - dist²*weight, gap)` — same curve as the CPU CRT shader (and lottes GL path).
#[inline]
#[must_use]
pub fn calc_scanline_weight(dist: f32, params: &CrtPiParams) -> f32 {
    (1.0 - dist * dist * params.scanline_weight).max(params.scanline_gap)
}

/// Scanline with optional 3-tap multisample (`filterWidth` from shader).
/// When `filter_width == 0`, multisample collapses to single tap.
#[inline]
#[must_use]
pub fn calc_scanline(dy: f32, filter_width: f32, params: &CrtPiParams) -> f32 {
    let mut w = calc_scanline_weight(dy, params);
    // MULTISAMPLE path: shader does two extra taps at ±filterWidth.
    if filter_width > 0.0 {
        w += calc_scanline_weight(dy - filter_width, params);
        w += calc_scanline_weight(dy + filter_width, params);
        w *= 0.333_333_3;
    }
    w
}

/// Per-row scanline brightness for a given pixel `y` within the glass.
/// Mirrors the fragment shader's `dy = texcoordInPixels.y - (floor(y)+0.5)` logic
/// but applied to whole rows for SDL line drawing. `texture_size_y` ≈ glass_h,
/// `output_size_y` ≈ window_h for filter width.
/// `vertical_scale` is the number of output rows per texture row (OutputSize.y / TextureSize.y);
/// 1.0 preserves the original behaviour where `TextureSize.y == glass_h`.
#[must_use]
pub fn scanline_weight_for_row(
    y: i32,
    glass_y: i32,
    params: &CrtPiParams,
    filter_width: f32,
    vertical_scale: f32,
) -> f32 {
    // Emulate `texcoordInPixels.y = (y - glass_y) / vertical_scale + 0.5`.
    // When TextureSize == glass_h, vertical_scale == 1.0 and this collapses to `y - glass_y + 0.5`.
    let scale = if vertical_scale <= 0.0 || !vertical_scale.is_finite() {
        1.0
    } else {
        vertical_scale
    };
    let fy = crate::constants::i32_to_f32(y - glass_y) / scale + 0.5;
    let temp_y = fy.floor() + 0.5;
    let dy = fy - temp_y;
    let mut w = calc_scanline(dy, filter_width, params);
    w *= params.bloom_factor;
    // Clamp to shader's implicit range after bloom (gap is already floor).
    w.clamp(params.scanline_gap * params.bloom_factor, params.bloom_factor)
}

/// Backward-compatible wrapper preserving existing call sites (scale = 1.0).
#[allow(dead_code)]
#[must_use]
pub fn scanline_weight_for_row_compat(y: i32, glass_y: i32, params: &CrtPiParams, filter_width: f32) -> f32 {
    scanline_weight_for_row(y, glass_y, params, filter_width, 1.0)
}

/// Compute `filterWidth = (InputSize.y / OutputSize.y) / 3` (as in the shader).
#[must_use]
pub fn filter_width(input_h: f32, output_h: f32) -> f32 {
    if output_h <= 0.0 {
        0.0
    } else {
        (input_h / output_h) / 3.0
    }
}

/// Gamma-correct a linear RGB triple (0..1) to output gamma.
#[allow(dead_code)]
#[must_use]
pub fn gamma_out(colour: [f32; 3], params: &CrtPiParams) -> [f32; 3] {
    let inv = 1.0 / params.output_gamma;
    [
        colour[0].powf(inv),
        colour[1].powf(inv),
        colour[2].powf(inv),
    ]
}

/// Apply input gamma (linearize) — mirrors `pow(colour, INPUT_GAMMA)`.
#[allow(dead_code)]
#[must_use]
pub fn gamma_in(colour: [f32; 3], params: &CrtPiParams) -> [f32; 3] {
    [
        colour[0].powf(params.input_gamma),
        colour[1].powf(params.input_gamma),
        colour[2].powf(params.input_gamma),
    ]
}

/// Barrel distortion (curvature) — maps 0..1 UV through radial warp.
/// `coord` in 0..1, `screen_scale = TextureSize / InputSize`.
/// Returns `None` if out-of-bounds (shader would discard).
#[must_use]
pub fn distort(coord: (f32, f32), screen_scale: (f32, f32), params: &CrtPiParams) -> Option<(f32, f32)> {
    let mut x = coord.0 * screen_scale.0 - 0.5;
    let mut y = coord.1 * screen_scale.1 - 0.5;
    let rsq = x * x + y * y;
    x += x * (params.curvature_x * rsq);
    y += y * (params.curvature_y * rsq);
    let barrel_scale_x = 1.0 - 0.23 * params.curvature_x;
    let barrel_scale_y = 1.0 - 0.23 * params.curvature_y;
    x *= barrel_scale_x;
    y *= barrel_scale_y;
    if x.abs() >= 0.5 || y.abs() >= 0.5 {
        return None;
    }
    x += 0.5;
    y += 0.5;
    x /= screen_scale.0;
    y /= screen_scale.1;
    Some((x, y))
}

/// Distort a pixel point inside the glass rect to its barrel-warped position.
/// Returns `None` when the warped point falls outside the glass (off-screen).
#[must_use]
pub fn distort_point(
    px: f32,
    py: f32,
    glass_x: f32,
    glass_y: f32,
    glass_w: f32,
    glass_h: f32,
    params: &CrtPiParams,
) -> Option<(f32, f32)> {
    if glass_w <= 0.0 || glass_h <= 0.0 {
        return None;
    }
    let nx = (px - glass_x) / glass_w;
    let ny = (py - glass_y) / glass_h;
    let (dx, dy) = distort((nx, ny), (1.0, 1.0), params)?;
    Some((glass_x + dx * glass_w, glass_y + dy * glass_h))
}

/// Horizontal warp offset (pixels) for a given `y` inside the glass.
/// Used by the CPU fallback to bow scanlines and offset text rows.
#[allow(dead_code)]
#[must_use]
pub fn curvature_offset_x(y: f32, glass_y: f32, glass_h: f32, params: &CrtPiParams) -> f32 {
    if glass_h <= 0.0 {
        return 0.0;
    }
    let ny = (y - glass_y) / glass_h - 0.5;
    let rsq = ny * ny;
    // Approximate barrel expansion at this vertical distance.
    params.curvature_x * rsq * 18.0
}

/// Shadow mask value for a given screen x (pixel coordinate), matching
/// `MASK_TYPE 1` (green/magenta) or `2` (Trinitron). Returns RGB multiplier.
#[allow(dead_code)]
#[must_use]
pub fn mask_for_x(x: f32, mask_type: u8, mask_brightness: f32) -> [f32; 3] {
    match mask_type {
        1 => {
            let which = (x * 0.5).fract();
            if which < 0.5 {
                [mask_brightness, 1.0, mask_brightness]
            } else {
                [1.0, mask_brightness, 1.0]
            }
        }
        2 => {
            let which = (x * 0.333_333_3).fract();
            if which < 0.333_333_3 {
                [1.0, mask_brightness, mask_brightness]
            } else if which < 0.666_666_6 {
                [mask_brightness, 1.0, mask_brightness]
            } else {
                [mask_brightness, mask_brightness, 1.0]
            }
        }
        _ => [1.0, 1.0, 1.0],
    }
}

/// Vignette factor (0..1) — used for CPU curvature approximation.
/// Stronger than a subtle `0.6..1.0` range so the bulge edge is obvious;
/// corners fall to ≈`0.35` instead of `0.6`.
#[must_use]
pub fn vignette_factor(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    glass_x: f32,
    glass_y: f32,
) -> f32 {
    let nx = ((x - glass_x) / w - 0.5) * 2.0;
    let ny = ((y - glass_y) / h - 0.5) * 2.0;
    let d = (nx * nx + ny * ny).sqrt();
    // Stronger falloff: 1 at center, ≈0.4 at corners, with a soft knee.
    (1.0 - d * 0.30 - d * d * 0.08).clamp(0.35, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanline_weight_at_center_is_one_times_bloom() {
        let p = CrtPiParams::default();
        let w = calc_scanline(0.0, 0.0, &p);
        assert!((w - 1.0).abs() < 1e-5);
    }

    #[test]
    fn scanline_weight_far_is_gap() {
        let p = CrtPiParams::default();
        let w = calc_scanline_weight(2.0, &p);
        assert!((w - p.scanline_gap).abs() < 1e-5);
    }

    #[test]
    fn distort_center_is_identity() {
        let p = CrtPiParams::default();
        let out = distort((0.5, 0.5), (1.0, 1.0), &p).unwrap();
        assert!((out.0 - 0.5).abs() < 1e-4);
        assert!((out.1 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn distort_corner_maybe_out_of_bounds_with_high_curvature() {
        let p = CrtPiParams {
            curvature_x: 1.0,
            curvature_y: 1.0,
            ..Default::default()
        };
        // corner may go out of bounds, but should not panic
        let _ = distort((0.0, 0.0), (1.0, 1.0), &p);
    }

    #[test]
    fn mask_type_0_is_identity() {
        let m = mask_for_x(10.0, 0, 0.7);
        assert_eq!(m, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn filter_width_formula() {
        assert!((filter_width(240.0, 1080.0) - 240.0 / 1080.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn default_curvature_is_visible() {
        // Must be obviously barrel-shaped on 80×24; 0.10 is too subtle.
        assert!((CURVATURE_X - 0.20).abs() < 1e-6);
        assert!((CURVATURE_Y - 0.20).abs() < 1e-6);
        let p = CrtPiParams::default();
        assert!((p.curvature_x - 0.20).abs() < 1e-6);
        assert!((p.curvature_y - 0.20).abs() < 1e-6);
    }

    #[test]
    fn vignette_is_strong_at_corners() {
        let v_center = vignette_factor(512.0, 382.0, 1024.0, 764.0, 0.0, 0.0);
        let v_corner = vignette_factor(0.0, 0.0, 1024.0, 764.0, 0.0, 0.0);
        assert!(v_center > 0.95, "center should be ~1.0, got {v_center}");
        assert!(v_corner < 0.60, "corner should be <=0.60 for obvious bulge, got {v_corner}");
        assert!(v_corner >= 0.35);
    }

    #[test]
    fn distort_point_bows_outward() {
        let p = CrtPiParams::default();
        // Center should map to itself
        let mid = distort_point(512.0, 382.0, 0.0, 0.0, 1024.0, 764.0, &p).unwrap();
        assert!((mid.0 - 512.0).abs() < 1.0);
        // Point near right edge at vertical center: shader's Distort is
        // screen->texture (outward for sampling), so the raw distorted point
        // is inset by barrelScale. For forward geometry (CPU text) we use
        // `-dx`, so middle must appear wider than top/bottom.
        let off_mid = distort_point(900.0, 382.0, 0.0, 0.0, 1024.0, 764.0, &p).unwrap();
        assert!(off_mid.0 < 900.0, "barrel inset: {off_mid:?} should be <900");
        assert!(off_mid.0 > 860.0, "inset shouldn't be excessive: {off_mid:?}");
        // Barrel: middle scanline/text must be wider than top/bottom.
        // off_mid is the raw screen->texture point; render.rs uses `-dx`
        // where dx = off - orig, so effective screen position is orig - dx.
        // Check that effective width at middle exceeds that at extreme top.
        let off_top = distort_point(900.0, 10.0, 0.0, 0.0, 1024.0, 764.0, &p).unwrap();
        let eff_mid = 900.0 - (off_mid.0 - 900.0);
        let eff_top = 900.0 - (off_top.0 - 900.0);
        assert!(
            eff_mid > eff_top,
            "barrel should be wider at middle: eff_mid {eff_mid} vs eff_top {eff_top} (raw mid {off_mid:?} top {off_top:?})"
        );
    }
}
