use sdl2::pixels::Color;

pub const COLS: usize = 80;
pub const ROWS: usize = 24;

pub const WINDOW_W: u32 = 1120;
pub const WINDOW_H: u32 = 860;
pub const BEZEL: i32 = 48;
pub const INNER_PAD: i32 = 14;

pub const BEZEL_COLOR: Color = Color::RGB(0x2e, 0x2e, 0x2a);
pub const BEZEL_HILITE: Color = Color::RGB(0x4a, 0x4a, 0x44);
pub const BEZEL_SHADOW: Color = Color::RGB(0x1a, 0x1a, 0x18);
pub const GLASS_BG: Color = Color::RGB(0x0a, 0x14, 0x0e);
pub const PHOSPHOR: Color = Color::RGB(0x33, 0xff, 0x66);
pub const PHOSPHOR_DIM: Color = Color::RGB(0x1a, 0xcc, 0x44);
pub const PHOSPHOR_BLOOM: Color = Color::RGB(0x66, 0xff, 0x99);
#[allow(dead_code)]
pub const AMBER: Color = Color::RGB(0xff, 0xcc, 0x33);

/// Convert `u32` window dimensions to `i32` without `as`.
/// Values are well below `i32::MAX`, so expect is safe.
pub fn window_w_i32() -> i32 {
    i32::try_from(WINDOW_W).expect("WINDOW_W fits in i32")
}

pub fn window_h_i32() -> i32 {
    i32::try_from(WINDOW_H).expect("WINDOW_H fits in i32")
}

pub fn cols_u32() -> u32 {
    u32::try_from(COLS).expect("COLS fits in u32")
}

pub fn rows_u32() -> u32 {
    u32::try_from(ROWS).expect("ROWS fits in u32")
}

/// Safe `u32 -> i32` for dimensions known to be small.
pub fn u32_to_i32(v: u32) -> i32 {
    i32::try_from(v).expect("value fits in i32")
}

/// Safe `usize -> u32`.
pub fn usize_to_u32(v: usize) -> u32 {
    u32::try_from(v).expect("value fits in u32")
}

/// Safe `usize -> i32`.
pub fn usize_to_i32(v: usize) -> i32 {
    i32::try_from(v).expect("value fits in i32")
}

/// Safe `i32 -> u32` for non-negative values.
// Kept for API completeness; allow dead_code to keep warnings minimal if unused.
#[allow(dead_code)]
pub fn i32_to_u32(v: i32) -> u32 {
    u32::try_from(v).expect("value fits in u32")
}

/// Convert `f32` in 0..=255 to `u8` without `as` in caller.
/// This is one of the few places where a float->int truncation is unavoidable;
/// we isolate it here with a documented allow.
pub fn f32_to_u8_clamped(v: f32) -> u8 {
    // Clippy pedantic cast_possible_truncation / cast_sign_loss is unavoidable
    // for float->int; value is clamped to 0..=255 and rounded first.
    let clamped = v.clamp(0.0, 255.0).round();
    // SAFETY: clamped is in 0..=255, so truncation is intentional and lossless in range.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    {
        clamped as u8
    }
}

/// Convert `f32` small integer part to `u8` via clamp for alpha values.
// Kept for API completeness; not used in current render but preserved for future palette work.
#[allow(dead_code)]
pub fn alpha_f32_to_u8(v: f32) -> u8 {
    f32_to_u8_clamped(v)
}

/// Convert `i32` small value (0..255) to `u8` without `as` at call site.
pub fn i32_to_u8(v: i32) -> u8 {
    u8::try_from(v).expect("value fits in u8")
}

/// Convert small `usize * N` to `u8` where result is known < 255.
pub fn usize_mul_to_u8(a: usize, b: usize) -> u8 {
    let prod = a.checked_mul(b).expect("product fits in usize");
    u8::try_from(prod).expect("product fits in u8")
}

/// Convert `i32` (small screen coordinate) to `f32` without `as`.
/// Values are < 2048 so they fit in i16 and conversion via `f32::from(i16)` is lossless
/// and avoids `cast_precision_loss`. Falls back to `f32::from` via i16 clamp for larger values.
pub fn i32_to_f32(v: i32) -> f32 {
    if let Ok(small) = i16::try_from(v) {
        f32::from(small)
    } else {
        // For larger values still within 16M, precision loss is negligible for rendering.
        // Use documented allow as fallback.
        #[allow(clippy::cast_precision_loss)]
        {
            v as f32
        }
    }
}

/// Convert `f32` to `i32` via rounding with clamped range.
/// Isolated here because float->int is unavoidable for CRT flicker math.
pub fn f32_to_i32_round(v: f32) -> i32 {
    // SAFETY: used for small screen flicker alpha (~32..44) so range is tiny.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    {
        v.round() as i32
    }
}
