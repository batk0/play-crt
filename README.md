# PLAY-CRT

Z-machine + 95 BASIC games in a phosphor CRT — 80×24 VT323 with shader and bottom switches.

Runs any `.z3/.z5/.z8/.zip` via a pure-Rust Z-machine (vendored [encrusted], MIT) and 95 Python BASIC games (from [basic-computer-games](https://github.com/coding-horror/basic-computer-games)). CRT look: `crt-pi` shader (curvature, scanlines, vignette, bloom, flicker), dark plastic bezel, and an interactive bottom control strip (phosphor colour + toggles).

Strict **80×24** monospaced grid. **VT323** font bundled (OFL). No external `frotz` needed — single portable binary.

---

## Install

### Homebrew (tap) — coming soon

```bash
brew install batk0/tap/play-crt
play-crt
```

### Cargo

```bash
cargo install play-crt
play-crt
```

Or build from source:

```bash
# SDL2 deps
brew install sdl2 sdl2_ttf          # macOS
sudo apt install libsdl2-dev libsdl2-ttf-dev pkg-config build-essential  # Debian/Ubuntu

cargo build --release
./target/release/play-crt
```

### Windows

- Install Rust via https://rustup.rs
- Either install SDL2 + SDL2_ttf dev libs, or build with bundled SDL2:

```bash
# edit Cargo.toml: sdl2 = { version = "0.38", features = ["bundled", "ttf"] }
cargo build --release
.\target\release\play-crt.exe
```

Windows exe with `bundled` needs no separate SDL2 install.

---

## Usage

```bash
play-crt                      # text menu — pick Z-machine or BASIC catalog
play-crt --story path/to/game.z3
play-crt --story game.zip
play-crt zork1.z3              # positional arg also works
play-crt --help
play-crt --version
```

Without `--story`, PLAY-CRT shows the catalog menu. Downloaded games are marked `[Ready]`; others download on `Enter`. `--curvature 0.20` (or `0.15,0.20`) tunes barrel curvature; `DEBUG=1` prints shader/font diagnostics.

---

## Data dir

PLAY-CRT stores everything under the OS data dir (override with `PLAY_CRT_DATA_DIR`):

- **macOS:** `~/Library/Application Support/play-crt`
- **Linux:** `~/.local/share/play-crt`
- **Windows:** `%APPDATA%\play-crt` (Roaming, via `ProjectDirs::data_local_dir`)

```
play-crt/
├── stories/<id>/<filename>   # downloaded Z-machine stories
├── saves/<id>/slot1.sav      # 3 slots per game (Quetzal)
│         <id>/slot2.sav
│         <id>/slot3.sav
├── basic/<id>/<filename>     # downloaded Python BASIC games
├── downloads/                # .part staging (atomic rename)
└── config.json               # phosphor, toggles, last catalog
```

`assets/manifests/stories.json` and `basic.json` are bundled catalogs. Local `.z*`/`.py` files dropped into `stories/` or `basic/` appear automatically. Portable mode: place an empty `portable` file next to the exe (or set `PLAY_CRT_PORTABLE=1`) to use `exe_dir/data`.

---

## Controls

| Key | Action |
|-----|--------|
| `Left` / `Right` (or `H`/`L`) | Switch catalog (Z-machine ↔ BASIC) |
| `Up` / `Down` | Move selection |
| `Enter` | Download (if needed) or select — Z-machine shows slot picker, BASIC launches |
| `B` | Back (from slot picker to menu) |
| `1`–`9` | Jump to entry / pick slot 1–3 directly |
| `R` | Refresh catalog |
| `Q` | Quit |
| `Esc` | In game: quit → menu (no process exit) |

In-game (`Z-machine`): type + `Enter` to send, `Backspace` to edit, `Up`/`Down` for history (100 entries). `SAVE`/`RESTORE` use the **selected slot** (3 slots per game) via the slot picker — no Z-machine filename prompt. Quitting a game returns to the menu, not to the OS.

**Bottom switches** (mouse, bottom 48 px bezel bar): click the 3-way phosphor switch (Green | Amber | White) or the **CURVE** / **FLICKER** / **SCAN** toggles. Hover highlights. Power LED (far right) is red while a game is running. Clicks in menu and in game both persist to `config.json`.

---

## Requirements

- **SDL2** + **sdl2_ttf** — `brew install sdl2 sdl2_ttf` (macOS) / `libsdl2-dev libsdl2-ttf-dev` (Linux). Windows: bundled feature or manual dev libs.
- **python3** — required for the 95 BASIC games only (`python3 --version` must succeed). Z-machine games have no Python dependency.
- **Font VT323** — bundled at `assets/fonts/VT323-Regular.ttf` (SIL OFL 1.1, © Peter Hull). Also `assets/fonts/OFL.txt`. Falls back to system mono if missing; point size auto-fits 80×24.

---

## License

- **Code** — MIT (see `LICENSE`). `src/zmachine/*` is vendored from [encrusted](https://github.com/demille/encrusted) (MIT). `src/crt_pi.rs` (Rust port of crt-pi math) is MIT.
- **crt-pi shader** — `assets/shaders/crt-pi.vert` and `assets/shaders/crt-pi.frag` (derived from `crt-pi.glsl`, GPL-2.0+, © 2015-2016 davej, from [libretro/glsl-shaders](https://github.com/libretro/glsl-shaders)) are **GPL-2.0+**. See `assets/shaders/LICENSE.crt-pi`. Redistribution of the `.vert`/`.frag` verbatim must comply with GPL-2.0+.
- **VT323 font** — SIL Open Font License 1.1 (`assets/fonts/OFL.txt`).
- **Game stories** — not bundled; Infocom titles remain © Infocom. BASIC games are Unlicense (see manifests). Use only content you have rights to.
