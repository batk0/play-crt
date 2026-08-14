use std::env;

#[derive(Debug, Default)]
pub struct Cli {
    pub story_arg: Option<String>,
    pub show_help: bool,
    pub show_version: bool,
    pub curvature: Option<(f32, f32)>,
}

#[allow(clippy::many_single_char_names)]
fn parse_curvature(s: &str) -> Result<(f32, f32), String> {
    // Accepts "0.20" (uniform) or "0.15,0.20" or "0.15x0.20"
    let cleaned = s.replace('x', ",");
    let parts: Vec<&str> = cleaned.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [single] => {
            let val: f32 = single.parse().map_err(|_| format!("invalid --curvature value: {s} (expected float or \"X,Y\")"))?;
            if !(0.0..=1.0).contains(&val) {
                return Err(format!("--curvature {val} out of range 0.0..1.0"));
            }
            Ok((val, val))
        }
        [ax, by] => {
            let cur_x: f32 = ax.parse().map_err(|_| format!("invalid --curvature X: {ax}"))?;
            let cur_y: f32 = by.parse().map_err(|_| format!("invalid --curvature Y: {by}"))?;
            if !(0.0..=1.0).contains(&cur_x) || !(0.0..=1.0).contains(&cur_y) {
                return Err(format!("--curvature {cur_x},{cur_y} out of range 0.0..1.0"));
            }
            Ok((cur_x, cur_y))
        }
        _ => Err(format!("invalid --curvature value: {s} (use 0.20 or 0.15,0.20)")),
    }
}

pub fn parse_args() -> Result<Cli, String> {
    let mut story_arg: Option<String> = None;
    let mut show_help = false;
    let mut show_version = false;
    let mut curvature: Option<(f32, f32)> = None;
    let mut args = env::args().skip(1).peekable();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--help" | "-h" => show_help = true,
            "--version" | "-V" => show_version = true,
            "--story" => {
                if let Some(v) = args.next() {
                    story_arg = Some(v);
                } else {
                    return Err("--story requires a path".into());
                }
            }
            s if s.starts_with("--story=") => {
                story_arg = Some(s["--story=".len()..].to_string());
            }
            "--curvature" => {
                if let Some(v) = args.next() {
                    curvature = Some(parse_curvature(&v)?);
                } else {
                    return Err("--curvature requires a value (e.g. --curvature 0.20 or --curvature 0.15,0.20)".into());
                }
            }
            s if s.starts_with("--curvature=") => {
                let v = &s["--curvature=".len()..];
                curvature = Some(parse_curvature(v)?);
            }
            s if s.starts_with('-') => {
                return Err(format!("unknown flag: {s}"));
            }
            s => {
                if story_arg.is_none() {
                    story_arg = Some(s.to_string());
                }
            }
        }
    }
    Ok(Cli {
        story_arg,
        show_help,
        show_version,
        curvature,
    })
}

pub fn print_help() {
    println!(
        r"play-crt — CRT phosphor GUI for Z-machine and text games

USAGE:
    play-crt [OPTIONS] [STORY]
    cargo run -- [OPTIONS] [STORY]
    cargo run -- --story ./zork1.z3
    cargo run -- --story assets/stories/zork1.z3

OPTIONS:
    --story <PATH>   Path to .z3/.z5/.z8/.zip story file (also accepts positional arg)
    --curvature <F[,F]>  Barrel curvature X,Y (0.0..1.0). Single value sets both axes.
                         Default 0.20,0.20 (visible bulge). Use 0 to disable curvature
                         (0.10 is a subtle but valid curvature, not disabled).
                         Examples: --curvature 0.20  --curvature 0.15,0.20  --curvature=0
    --help, -h       Show this help
    --version, -V    Show version

STORY DISCOVERY:
    If --story not given, searches:
      ./zork1.z3, ./zork1.zip, ./assets/stories/*, ./stories/*, and ancestors.

STORY REQUIRED:
    No story is bundled. Provide --story <path> (e.g. from a historical
    zork1 checkout) or place a story at assets/stories/zork1.z3.

Z-MACHINE:
    Pure Rust interpreter (vendored encrusted, MIT) — no external frotz needed.
    Binary is self-contained and portable (supports .z3/.z5/.z8 and .zip archives).
    Screen is fixed 80×24, matching the CRT grid.

CONTROLS (in GUI):
    Type + Enter to send commands. Backspace to edit. Esc or close window to quit.
    The CRT renders a strict 80×24 monospaced grid with VT323, scanlines,
    vignette, bloom, flicker, curvature (via rounded glass + vignette) and a
    dark plastic bezel with screws and power LED.

EXAMPLES:
    cargo run -- --story ./zork1.z3
    cargo run -- --story assets/stories/zork1.z3
    cargo run -- zork1.zip
    ./target/debug/play-crt --story /path/to/game.z5
"
    );
}
