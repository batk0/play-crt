//! Interactive bezel controls — phosphor selector + effect toggles.
//!
//! The bottom bezel strip (`WINDOW_H - BEZEL .. WINDOW_H`) hosts:
//!
//! - a 3-position phosphor colour switch (Green / Amber / White)
//! - three toggle switches (curvature / flicker / scanlines)
//!
//! plus the existing power LED at far right.
//!
//! Layout is deterministic and shared between hit-testing (`main.rs`) and
//! rendering (`render.rs`) via `bottom_bar_layout()` so the two stay in sync.

use sdl2::pixels::Color;
use sdl2::rect::Rect;

use crate::constants::{window_h_i32, window_w_i32, BEZEL};
use crate::crt_pi;

// ── Phosphor ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PhosphorColor {
    #[default]
    Green,
    Amber,
    White,
}

impl PhosphorColor {
    #[must_use]
    #[allow(dead_code)]
    pub const fn index(self) -> usize {
        match self {
            Self::Green => 0,
            Self::Amber => 1,
            Self::White => 2,
        }
    }

    #[must_use]
    pub const fn from_index(idx: usize) -> Self {
        match idx % 3 {
            0 => Self::Green,
            1 => Self::Amber,
            _ => Self::White,
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn next(self) -> Self {
        Self::from_index(self.index() + 1)
    }

    /// Main phosphor colour (text).
    #[must_use]
    pub const fn color(self) -> Color {
        match self {
            Self::Green => Color::RGB(0x33, 0xFF, 0x66),
            Self::Amber => Color::RGB(0xFF, 0xB0, 0x00),
            Self::White => Color::RGB(0xE0, 0xE0, 0xC0),
        }
    }

    /// Dim variant (cursor when inactive, etc.).
    #[must_use]
    pub const fn dim(self) -> Color {
        match self {
            Self::Green => Color::RGB(0x1A, 0xCC, 0x44),
            Self::Amber => Color::RGB(0xCC, 0x88, 0x00),
            Self::White => Color::RGB(0xB8, 0xB8, 0x9A),
        }
    }

    /// Bloom halo colour (lighter copy behind text).
    #[must_use]
    pub const fn bloom(self) -> Color {
        match self {
            Self::Green => Color::RGB(0x66, 0xFF, 0x99),
            Self::Amber => Color::RGB(0xFF, 0xCC, 0x66),
            Self::White => Color::RGB(0xFF, 0xFF, 0xE8),
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Green => "GREEN",
            Self::Amber => "AMBER",
            Self::White => "WHITE",
        }
    }

    /// String used in `config.json` (`Green`/`Amber`/`White`).
    #[must_use]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::Green => "Green",
            Self::Amber => "Amber",
            Self::White => "White",
        }
    }

    /// Parse `config.json` phosphor string (case-insensitive, defaults to Green).
    #[must_use]
    pub fn from_config_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("amber") {
            Self::Amber
        } else if s.eq_ignore_ascii_case("white") {
            Self::White
        } else {
            Self::Green
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Green => "G",
            Self::Amber => "A",
            Self::White => "W",
        }
    }
}

// ── ControlState ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct ControlState {
    pub phosphor: PhosphorColor,
    pub curvature_enabled: bool,
    pub flicker_enabled: bool,
    pub scanlines_enabled: bool,
}

impl Default for ControlState {
    fn default() -> Self {
        Self {
            phosphor: PhosphorColor::Green,
            curvature_enabled: true,
            flicker_enabled: true,
            scanlines_enabled: true,
        }
    }
}

impl ControlState {
    #[must_use]
    pub fn phosphor_color(self) -> Color {
        self.phosphor.color()
    }

    #[must_use]
    pub fn phosphor_dim(self) -> Color {
        self.phosphor.dim()
    }

    #[must_use]
    pub fn phosphor_bloom(self) -> Color {
        self.phosphor.bloom()
    }

    /// `CrtPiParams` with curvature zeroed when the toggle is off.
    #[must_use]
    pub fn crt_params(self) -> crt_pi::CrtPiParams {
        let mut p = crt_pi::CrtPiParams::default();
        if !self.curvature_enabled {
            p.curvature_x = 0.0;
            p.curvature_y = 0.0;
        }
        p
    }
}

// ── Layout ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct PhosphorLayout {
    pub track: Rect,
    pub segments: [Rect; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct ToggleLayout {
    pub track: Rect,
}

#[derive(Clone, Copy, Debug)]
pub struct BottomBarLayout {
    pub bar_y: i32,
    pub center_y: i32,
    pub phosphor: PhosphorLayout,
    pub curvature: ToggleLayout,
    pub flicker: ToggleLayout,
    pub scanlines: ToggleLayout,
}

/// Deterministic layout for the bottom bezel bar.
///
/// Centred group: `[ PHOSPHOR ]  [CURV] [FLICKER] [SCAN]` + LED far right.
/// All coordinates are window-space `i32` so hit-test and rendering agree.
#[must_use]
pub fn bottom_bar_layout() -> BottomBarLayout {
    let ww = window_w_i32();
    let wh = window_h_i32();
    let bar_y = wh - BEZEL;
    let center_y = bar_y + BEZEL / 2;

    // Group centres — spaced evenly around window centre.
    let cx_phosphor = ww / 2 - 210;
    let cx_curv = ww / 2 - 70;
    let cx_flicker = ww / 2 + 70;
    let cx_scan = ww / 2 + 210;

    // Phosphor: 96×20 track split into 3×32 segments.
    let ph_track = Rect::new(cx_phosphor - 48, center_y - 10, 96, 20);
    let seg_w: u32 = 32;
    let seg_h: u32 = 20;
    let seg_y = center_y - 10;
    let segments = [
        Rect::new(cx_phosphor - 48, seg_y, seg_w, seg_h),
        Rect::new(cx_phosphor - 16, seg_y, seg_w, seg_h),
        Rect::new(cx_phosphor + 16, seg_y, seg_w, seg_h),
    ];

    // Toggles: 36×18 tracks.
    let curv_track = Rect::new(cx_curv - 18, center_y - 9, 36, 18);
    let flicker_track = Rect::new(cx_flicker - 18, center_y - 9, 36, 18);
    let scan_track = Rect::new(cx_scan - 18, center_y - 9, 36, 18);

    BottomBarLayout {
        bar_y,
        center_y,
        phosphor: PhosphorLayout {
            track: ph_track,
            segments,
        },
        curvature: ToggleLayout { track: curv_track },
        flicker: ToggleLayout { track: flicker_track },
        scanlines: ToggleLayout { track: scan_track },
    }
}

// ── Hit testing ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlHit {
    Phosphor(PhosphorColor),
    Curvature,
    Flicker,
    Scanlines,
}

/// Hit-test a window-space point against the bottom bar controls.
#[must_use]
pub fn hit_test(x: i32, y: i32) -> Option<ControlHit> {
    let layout = bottom_bar_layout();
    let wh = window_h_i32();
    if y < layout.bar_y || y >= wh {
        return None;
    }
    // Phosphor — check each segment expanded by 2px for forgiving hit area.
    for (idx, seg) in layout.phosphor.segments.iter().enumerate() {
        let expanded = Rect::new(seg.x() - 2, seg.y() - 4, seg.width() + 4, seg.height() + 8);
        if expanded.contains_point((x, y)) {
            return Some(ControlHit::Phosphor(PhosphorColor::from_index(idx)));
        }
    }
    // Fallback: clicking anywhere on the phosphor track selects nearest segment.
    if layout.phosphor.track.contains_point((x, y)) {
        let rel = x - layout.phosphor.track.x();
        let idx = if rel < 32 {
            0
        } else if rel < 64 {
            1
        } else {
            2
        };
        return Some(ControlHit::Phosphor(PhosphorColor::from_index(idx)));
    }
    // Toggles — generous hit area (track ±6px).
    let check_toggle = |r: Rect| {
        let expanded = Rect::new(r.x() - 6, r.y() - 6, r.width() + 12, r.height() + 12);
        expanded.contains_point((x, y))
    };
    if check_toggle(layout.curvature.track) {
        return Some(ControlHit::Curvature);
    }
    if check_toggle(layout.flicker.track) {
        return Some(ControlHit::Flicker);
    }
    if check_toggle(layout.scanlines.track) {
        return Some(ControlHit::Scanlines);
    }
    None
}

/// Apply a click to `state`. Returns `true` if the click was handled.
pub fn handle_click(state: &mut ControlState, x: i32, y: i32) -> bool {
    match hit_test(x, y) {
        Some(ControlHit::Phosphor(c)) => {
            state.phosphor = c;
            true
        }
        Some(ControlHit::Curvature) => {
            state.curvature_enabled = !state.curvature_enabled;
            true
        }
        Some(ControlHit::Flicker) => {
            state.flicker_enabled = !state.flicker_enabled;
            true
        }
        Some(ControlHit::Scanlines) => {
            state.scanlines_enabled = !state.scanlines_enabled;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phosphor_colors_distinct() {
        assert_ne!(PhosphorColor::Green.color(), PhosphorColor::Amber.color());
        assert_ne!(PhosphorColor::Amber.color(), PhosphorColor::White.color());
    }

    #[test]
    fn amber_is_ffb000() {
        assert_eq!(PhosphorColor::Amber.color(), Color::RGB(0xFF, 0xB0, 0x00));
    }

    #[test]
    fn hit_test_phosphor_segments() {
        let layout = bottom_bar_layout();
        let seg0_center_x = layout.phosphor.segments[0].x() + 16;
        let y = layout.center_y;
        assert_eq!(
            hit_test(seg0_center_x, y),
            Some(ControlHit::Phosphor(PhosphorColor::Green))
        );
        let seg2_center_x = layout.phosphor.segments[2].x() + 16;
        assert_eq!(
            hit_test(seg2_center_x, y),
            Some(ControlHit::Phosphor(PhosphorColor::White))
        );
    }

    #[test]
    fn hit_test_toggles() {
        let layout = bottom_bar_layout();
        let c = layout.curvature.track;
        assert_eq!(
            hit_test(c.x() + 5, c.y() + 5),
            Some(ControlHit::Curvature)
        );
        let s = layout.scanlines.track;
        assert_eq!(
            hit_test(s.x() + 5, s.y() + 5),
            Some(ControlHit::Scanlines)
        );
    }

    #[test]
    fn outside_bar_no_hit() {
        assert_eq!(hit_test(100, 10), None);
    }

    #[test]
    fn handle_click_toggles() {
        let mut s = ControlState::default();
        let layout = bottom_bar_layout();
        let c = layout.curvature.track;
        assert!(s.curvature_enabled);
        let _ = handle_click(&mut s, c.x() + 5, c.y() + 5);
        assert!(!s.curvature_enabled);
    }

    #[test]
    fn curvature_params_zero_when_disabled() {
        let mut s = ControlState::default();
        s.curvature_enabled = false;
        let p = s.crt_params();
        assert_eq!(p.curvature_x, 0.0);
        assert_eq!(p.curvature_y, 0.0);
    }
}
