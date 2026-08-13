use std::env;

#[derive(Debug, Default)]
pub struct Cli {
    pub story_arg: Option<String>,
    pub show_help: bool,
    pub show_version: bool,
}

pub fn parse_args() -> Result<Cli, String> {
    let mut story_arg: Option<String> = None;
    let mut show_help = false;
    let mut show_version = false;
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
    })
}

pub fn print_help() {
    println!(
        r"zork-crt-gui — CRT phosphor GUI for Z-machine games

USAGE:
    zork-crt-gui [OPTIONS] [STORY]
    cargo run -- [OPTIONS] [STORY]
    cargo run -- --story ./zork1.z3
    cargo run -- --story assets/stories/zork1.z3

OPTIONS:
    --story <PATH>   Path to .z3/.z5/.z8/.zip story file (also accepts positional arg)
    --help, -h       Show this help
    --version        Show version

STORY DISCOVERY:
    If --story not given, searches:
      ./zork1.z3, ./zork1.zip, ./assets/stories/*, ./stories/*, and ancestors.

STORY REQUIRED:
    No story is bundled. Provide --story <path> (e.g. from a historical
    zork1 checkout) or place a story at assets/stories/zork1.z3.

DFROTZ:
    Requires 'dfrotz' (dumb frotz). Install via:
      brew install frotz
    Looked up at /opt/homebrew/bin/dfrotz, /usr/local/bin/dfrotz, and $PATH.
    If missing, GUI shows an error screen with instructions.

CONTROLS (in GUI):
    Type + Enter to send commands. Backspace to edit. Esc or close window to quit.
    The CRT renders a strict 80×24 monospaced grid with VT323, scanlines,
    vignette, bloom, flicker, curvature (via rounded glass + vignette) and a
    dark plastic bezel with screws and power LED.

EXAMPLES:
    cargo run -- --story ./zork1.z3
    cargo run -- --story assets/stories/zork1.z3
    cargo run -- zork1.zip
     ./target/debug/zork-crt-gui --story /path/to/game.z5
"
    );
}
