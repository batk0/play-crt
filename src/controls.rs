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

// ── Baud ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BaudRate {
    Baud110,
    Baud300,
    Baud1200,
    #[default]
    Baud2400,
    Baud9600,
    Infinity,
}

impl BaudRate {
    #[allow(dead_code)]
    pub const ALL: [Self; 6] = [
        Self::Baud110,
        Self::Baud300,
        Self::Baud1200,
        Self::Baud2400,
        Self::Baud9600,
        Self::Infinity,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Baud110 => "110",
            Self::Baud300 => "300",
            Self::Baud1200 => "1200",
            Self::Baud2400 => "2400",
            Self::Baud9600 => "9600",
            Self::Infinity => "INF",
        }
    }

    #[must_use]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::Baud110 => "110",
            Self::Baud300 => "300",
            Self::Baud1200 => "1200",
            Self::Baud2400 => "2400",
            Self::Baud9600 => "9600",
            Self::Infinity => "infinity",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "110" => Some(Self::Baud110),
            "300" => Some(Self::Baud300),
            "1200" => Some(Self::Baud1200),
            "2400" => Some(Self::Baud2400),
            "9600" => Some(Self::Baud9600),
            "infinity" | "inf" | "∞" | "0" => Some(Self::Infinity),
            _ => None,
        }
    }

    #[must_use]
    pub fn from_config_str(s: &str) -> Self {
        Self::from_str(s).unwrap_or(Self::Baud2400)
    }

    /// Chars per second (10 bits per char).
    #[allow(dead_code)]
    #[must_use]
    pub fn cps(self) -> f32 {
        match self {
            Self::Baud110 => 11.0,
            Self::Baud300 => 30.0,
            Self::Baud1200 => 120.0,
            Self::Baud2400 => 240.0,
            Self::Baud9600 => 960.0,
            Self::Infinity => f32::INFINITY,
        }
    }

    /// Interval per character (10/baud secs). `None` for infinity (no delay).
    #[must_use]
    pub fn interval(self) -> Option<std::time::Duration> {
        match self {
            Self::Baud110 => Some(std::time::Duration::from_secs_f64(10.0 / 110.0)),
            Self::Baud300 => Some(std::time::Duration::from_secs_f64(10.0 / 300.0)),
            Self::Baud1200 => Some(std::time::Duration::from_secs_f64(10.0 / 1200.0)),
            Self::Baud2400 => Some(std::time::Duration::from_secs_f64(10.0 / 2400.0)),
            Self::Baud9600 => Some(std::time::Duration::from_secs_f64(10.0 / 9600.0)),
            Self::Infinity => None,
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Baud110 => Self::Baud300,
            Self::Baud300 => Self::Baud1200,
            Self::Baud1200 => Self::Baud2400,
            Self::Baud2400 => Self::Baud9600,
            Self::Baud9600 => Self::Infinity,
            Self::Infinity => Self::Baud110,
        }
    }
}



// ── Sound ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SoundPalette {
    #[default]
    Teletype,
    ModemCrt,
    Minimal,
}

impl SoundPalette {
    #[allow(dead_code)]
    pub const ALL: [Self; 3] = [Self::Teletype, Self::ModemCrt, Self::Minimal];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Teletype => "TTY",
            Self::ModemCrt => "MODEM",
            Self::Minimal => "MIN",
        }
    }

    #[must_use]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::Teletype => "Teletype",
            Self::ModemCrt => "ModemCrt",
            Self::Minimal => "Minimal",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "teletype" | "tty" | "typewriter" => Some(Self::Teletype),
            "modem" | "modemcrt" | "modem_crt" | "crt" => Some(Self::ModemCrt),
            "minimal" | "min" | "quiet" | "silent" => Some(Self::Minimal),
            _ => None,
        }
    }

    #[must_use]
    pub fn from_config_str(s: &str) -> Self {
        Self::from_str(s).unwrap_or(Self::Teletype)
    }

    #[allow(dead_code)]
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Teletype => Self::ModemCrt,
            Self::ModemCrt => Self::Minimal,
            Self::Minimal => Self::Teletype,
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Teletype => 0,
            Self::ModemCrt => 1,
            Self::Minimal => 2,
        }
    }

    #[must_use]
    pub const fn from_index(idx: usize) -> Self {
        match idx % 3 {
            0 => Self::Teletype,
            1 => Self::ModemCrt,
            _ => Self::Minimal,
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
    pub baud_rate: BaudRate,
    pub sound_palette: SoundPalette,
}

impl Default for ControlState {
    fn default() -> Self {
        Self {
            phosphor: PhosphorColor::Green,
            curvature_enabled: true,
            flicker_enabled: true,
            scanlines_enabled: true,
            baud_rate: BaudRate::Baud2400,
            sound_palette: SoundPalette::Teletype,
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

    #[must_use]
    pub fn sound_palette_label_color(self) -> Color {
        match self.sound_palette {
            SoundPalette::Teletype => Color::RGB(0x66, 0xFF, 0x99),
            SoundPalette::ModemCrt => Color::RGB(0x88, 0xCC, 0xFF),
            SoundPalette::Minimal => Color::RGB(0xCC, 0xCC, 0xC0),
        }
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
pub struct SoundLayout {
    pub track: Rect,
    pub segments: [Rect; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct BottomBarLayout {
    pub bar_y: i32,
    pub center_y: i32,
    pub baud: ToggleLayout,
    pub phosphor: PhosphorLayout,
    pub curvature: ToggleLayout,
    pub flicker: ToggleLayout,
    pub scanlines: ToggleLayout,
    pub sound: SoundLayout,
}

/// Deterministic layout for the bottom bezel bar.
///
/// Centred group: `[BAUD] [ PHOSPHOR ]  [CURV] [FLICKER] [SCAN] [ SOUND ]` + LED far right.
/// All coordinates are window-space `i32` so hit-test and rendering agree.
/// Existing centres for phosphor/curvature/flicker/scan are preserved to keep
/// hit-tests stable; baud and sound are added at outer positions.
#[must_use]
pub fn bottom_bar_layout() -> BottomBarLayout {
    let ww = window_w_i32();
    let wh = window_h_i32();
    let bar_y = wh - BEZEL;
    let center_y = bar_y + BEZEL / 2;

    // Group centres — original four preserved, two new at outer edges.
    let cx_baud = ww / 2 - 350;
    let cx_phosphor = ww / 2 - 210;
    let cx_curv = ww / 2 - 70;
    let cx_flicker = ww / 2 + 70;
    let cx_scan = ww / 2 + 210;
    let cx_sound = ww / 2 + 350;

    // Phosphor: 96×20 track split into 3×32 segments.
    let ph_track = Rect::new(cx_phosphor - 48, center_y - 10, 96, 20);
    let seg_w: u32 = 32;
    let seg_h: u32 = 20;
    let seg_y = center_y - 10;
    let ph_segments = [
        Rect::new(cx_phosphor - 48, seg_y, seg_w, seg_h),
        Rect::new(cx_phosphor - 16, seg_y, seg_w, seg_h),
        Rect::new(cx_phosphor + 16, seg_y, seg_w, seg_h),
    ];

    // Baud: single cycle button 64×20
    let baud_track = Rect::new(cx_baud - 32, center_y - 10, 64, 20);

    // Toggles: 36×18 tracks.
    let curv_track = Rect::new(cx_curv - 18, center_y - 9, 36, 18);
    let flicker_track = Rect::new(cx_flicker - 18, center_y - 9, 36, 18);
    let scan_track = Rect::new(cx_scan - 18, center_y - 9, 36, 18);

    // Sound: 96×20 track split into 3×32 segments (TTY / MODEM / MIN).
    let sound_track = Rect::new(cx_sound - 48, center_y - 10, 96, 20);
    let sound_segments = [
        Rect::new(cx_sound - 48, seg_y, seg_w, seg_h),
        Rect::new(cx_sound - 16, seg_y, seg_w, seg_h),
        Rect::new(cx_sound + 16, seg_y, seg_w, seg_h),
    ];

    BottomBarLayout {
        bar_y,
        center_y,
        baud: ToggleLayout { track: baud_track },
        phosphor: PhosphorLayout {
            track: ph_track,
            segments: ph_segments,
        },
        curvature: ToggleLayout { track: curv_track },
        flicker: ToggleLayout { track: flicker_track },
        scanlines: ToggleLayout { track: scan_track },
        sound: SoundLayout {
            track: sound_track,
            segments: sound_segments,
        },
    }
}

// ── Hit testing ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlHit {
    Phosphor(PhosphorColor),
    Curvature,
    Flicker,
    Scanlines,
    Baud,
    Sound(SoundPalette),
}

/// Hit-test a window-space point against the bottom bar controls.
#[must_use]
pub fn hit_test(x: i32, y: i32) -> Option<ControlHit> {
    let layout = bottom_bar_layout();
    let wh = window_h_i32();
    if y < layout.bar_y || y >= wh {
        return None;
    }
    // Baud — single button
    {
        let r = layout.baud.track;
        let expanded = Rect::new(r.x() - 6, r.y() - 4, r.width() + 12, r.height() + 8);
        if expanded.contains_point((x, y)) {
            return Some(ControlHit::Baud);
        }
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
    // Sound — check each segment
    for (idx, seg) in layout.sound.segments.iter().enumerate() {
        let expanded = Rect::new(seg.x() - 2, seg.y() - 4, seg.width() + 4, seg.height() + 8);
        if expanded.contains_point((x, y)) {
            return Some(ControlHit::Sound(SoundPalette::from_index(idx)));
        }
    }
    if layout.sound.track.contains_point((x, y)) {
        let rel = x - layout.sound.track.x();
        let idx = if rel < 32 {
            0
        } else if rel < 64 {
            1
        } else {
            2
        };
        return Some(ControlHit::Sound(SoundPalette::from_index(idx)));
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
        Some(ControlHit::Baud) => {
            state.baud_rate = state.baud_rate.next();
            true
        }
        Some(ControlHit::Sound(p)) => {
            state.sound_palette = p;
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
        let s = ControlState {
            curvature_enabled: false,
            ..Default::default()
        };
        let p = s.crt_params();
        assert_eq!(p.curvature_x, 0.0);
        assert_eq!(p.curvature_y, 0.0);
    }

    #[test]
    fn baud_cycle() {
        assert_eq!(BaudRate::Baud2400.next(), BaudRate::Baud9600);
        assert_eq!(BaudRate::Infinity.next(), BaudRate::Baud110);
        assert_eq!(BaudRate::from_str("2400"), Some(BaudRate::Baud2400));
        assert_eq!(BaudRate::from_str("infinity"), Some(BaudRate::Infinity));
        assert_eq!(BaudRate::from_str("INF"), Some(BaudRate::Infinity));
        assert!(BaudRate::Baud2400.interval().is_some());
        assert!(BaudRate::Infinity.interval().is_none());
        assert!(BaudRate::Baud110.cps() < BaudRate::Baud9600.cps());
    }

    #[test]
    fn sound_palette_cycle() {
        assert_eq!(SoundPalette::Teletype.next(), SoundPalette::ModemCrt);
        assert_eq!(SoundPalette::Minimal.next(), SoundPalette::Teletype);
        assert_eq!(SoundPalette::from_str("teletype"), Some(SoundPalette::Teletype));
        assert_eq!(SoundPalette::from_str("modem"), Some(SoundPalette::ModemCrt));
        assert_eq!(SoundPalette::from_str("minimal"), Some(SoundPalette::Minimal));
        assert_eq!(SoundPalette::Teletype.label(), "TTY");
    }

    #[test]
    fn hit_test_baud_and_sound() {
        let layout = bottom_bar_layout();
        let baud_center = layout.baud.track.x() + 5;
        let y = layout.center_y;
        assert_eq!(hit_test(baud_center, y), Some(ControlHit::Baud));
        let sound_seg = layout.sound.segments[1].x() + 5;
        assert_eq!(
            hit_test(sound_seg, y),
            Some(ControlHit::Sound(SoundPalette::ModemCrt))
        );
    }

    #[test]
    fn handle_click_baud_and_sound() {
        let mut s = ControlState::default();
        let layout = bottom_bar_layout();
        let baud_x = layout.baud.track.x() + 5;
        let y = layout.center_y;
        let start = s.baud_rate;
        let _ = handle_click(&mut s, baud_x, y);
        assert_ne!(s.baud_rate, start);
        let sound_x = layout.sound.segments[2].x() + 5;
        let _ = handle_click(&mut s, sound_x, y);
        assert_eq!(s.sound_palette, SoundPalette::Minimal);
    }
}
