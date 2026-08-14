# Zork CRT GUI — SDL2 phosphor terminal for Z-machine games

Portable Rust + SDL2 CRT that runs **Zork I** (and any `.z3/.z5/.z8/.zip`) with a full phosphor-CRT look: curvature (rounded glass + vignette), scanlines, vignette, bloom, flicker, and a dark plastic bezel with power LED + bottom control strip (phosphor colour + toggles). Strict **80×24** monospaced grid, **VT323** font (bundled, OFL).

Backend is a **pure-Rust Z-machine** (vendored [encrusted](https://github.com/demille/encrusted), MIT) — no external `dfrotz`/`frotz` needed. The binary is self-contained MIT, portable, and works with any Z-machine story via `--story`. No story file is bundled — provide one with `--story <path>` or place it at `assets/stories/zork1.z3` / `./zork1.z3`.

> No Python. Single portable binary via `cargo build`. No `brew install frotz` required.

---

## Quick start

### macOS (Homebrew, Apple Silicon / Intel)

```bash
# 1) toolchain + SDL2
brew install sdl2 sdl2_ttf
# rustup if you don't have it
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# source cargo
source $HOME/.cargo/env

# 2) build (from repo root)
cargo build

# 3) run — story is REQUIRED (no story is bundled)
# pass an explicit path:
cargo run -- --story /path/to/zork1.z3
# or if you placed a story at assets/stories/zork1.z3 or ./zork1.z3:
cargo run -- --story assets/stories/zork1.z3
./target/debug/zork-crt-gui --story ./zork1.z3

# alternative: positional arg or file picker (F1)
cargo run -- zork1.zip
cargo run --              # opens native file picker (F1) or shows help screen
./target/debug/zork-crt-gui --story ~/games/enchanter.z5
```

> **Where to get a story file?** Build `zork1.z3` from the historical `zork1` repo (`zil` → `COMPILED/zork1.z3`), or use any `.z3/.z5/.z8/.zip` you own. Copy it to this repo if you like:
> ```bash
> mkdir -p assets/stories
> cp /path/to/COMPILED/zork1.z3 assets/stories/zork1.z3
> cargo run -- --story assets/stories/zork1.z3
> ```

**Linux (Debian/Ubuntu)**

```bash
sudo apt update
sudo apt install libsdl2-dev libsdl2-ttf-dev pkg-config build-essential
cargo build
./target/debug/zork-crt-gui --story /path/to/zork1.z3
```

**Windows (MSVC)**

- Install Rust via https://rustup.rs
- Install SDL2 + SDL2_ttf dev libs and add to `LIB`/`PATH`, or use `sdl2` crate's `bundled` feature: edit `Cargo.toml` to `sdl2 = { version="0.38", features=["ttf","bundled"] }` then `cargo build`.
- `cargo run -- --story path\to\zork1.z3` — no external frotz binary needed.

---

## Usage

```
zork-crt-gui [OPTIONS] [STORY]

OPTIONS:
  --story <PATH>   Path to .z3/.z5/.z8/.zip (also positional)
  --curvature <F[,F]>  Barrel curvature 0.0..1.0
  --help, -h
  --version

STORY DISCOVERY (if --story omitted):
  Searches ./zork1.z3, ./zork1.zip, ./assets/stories/*, ./stories/*,
  ancestors, and exe-relative paths.
  If still not found, GUI shows an error screen (press F1 for native file picker).
  No story is bundled — prefer passing --story explicitly.

EXAMPLES:
  cargo run -- --story ./zork1.z3
  cargo run -- --story assets/stories/zork1.z3
  cargo run -- --story /path/to/minizork.z3
  cargo run -- --story zork1.zip
  ./target/debug/zork-crt-gui --story ~/games/enchanter.z5
```

**In-GUI controls**

- Type → characters echo in phosphor colour on the 80×24 grid (Green / Amber / White via bottom switch)
- `Enter` → send line to the Z-machine
- `Backspace` → edit current line
- `Up`/`Down` → command history (100 entries)
- `F1` → native file picker for another story (restarts the Rust VM)
- `Esc` or close window → quit
- **Mouse (bottom 48 px bezel bar)** — click the 3-way phosphor switch (Green | Amber | White) to change text/bloom colour; click the three toggle switches to enable/disable **Curvature**, **Flicker**, and **Scanlines** live. Hover highlights the control under the cursor. Power LED (far right, red when Z-machine alive) is always visible.

---

## CRT effects — crt-pi shader (SDL2 + OpenGL)

This build integrates **[crt-pi](https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-pi.glsl)** — the Raspberry Pi-friendly CRT shader by `davej` (© 2015-2016) — as the reference scanline/curvature model.

- **Shader files** — `assets/shaders/crt-pi.glsl` (verbatim, GPL-2.0+), plus split `crt-pi.vert` / `crt-pi.frag` (GL 3.3 core) for the `glow` path. See `assets/shaders/LICENSE.crt-pi`.
- **Rust port** — `src/crt_pi.rs` re-implements `CalcScanLineWeight` / `CalcScanLine` / `MULTISAMPLE` / `BLOOM_FACTOR` / `MASK_TYPE` / `INPUT_GAMMA` / `OUTPUT_GAMMA` / `filterWidth` / barrel `Distort()` in pure Rust for the SDL2 CPU fallback. Algorithm is MIT; GLSL text remains GPL-2.0+.
- **GL path** — `src/crt_gl.rs` uses `glow = "0.16"` to compile the same GLSL at startup (`CrtGl::try_new(&video, &window)`). If a GL 3.3 core context is available the shader compiles and is kept alive; window presentation still goes through the SDL2 Canvas so the fallback is always valid. A future fullscreen-quad blit (`bind()` → draw) can consume the program without changing `grid`/`backend`.
- **CPU fallback (always active today)** — SDL2 2D renderer, but scanlines now use the true crt-pi formula: per-row `scanline_weight_for_row()` with `filterWidth = InputSize.y / OutputSize.y / 3` (3-tap multisample), `SCANLINE_WEIGHT=6.0`, `SCANLINE_GAP=0.12`, `BLOOM_FACTOR=1.5` (inverted to darkness so gaps are opaque, lines transparent), plus sinusoidal wobble. Aperture mask (`MASK_TYPE 1` green/magenta) is drawn as 1 px vertical stripes at low alpha; vignette + corner darkening + specular highlight + bezel glow approximate the curvature barrel that the shader does for free on the GPU.

Result: with GL you get a true fragment shader; without GL (headless, CI, any SDL2 target) you get a pixel-identical CPU approximation. Either way the **80×24 VT323 grid, phosphor bloom, flicker/hum** are preserved and the bottom bezel now hosts interactive controls.

**Bezel** — dark plastic outer frame, hilite/shadow edges, power LED (red when Z-machine alive), bottom control strip with phosphor colour switch + 3 effect toggles (no screws/speaker/brand plate).  
**Phosphor** — selectable **Green** `#33FF66` (`PHOSPHOR`), **Amber** `#FFB000`, **White** `#E0E0C0`; bloom/dim variants derived per colour (`controls::PhosphorColor`). Toggle via `ControlState` and `draw_bottom_controls`.  
**Curvature** — rounded glass illusion via inner shadow + vignette + top highlight; on GL via `Distort()` barrel math (`CURVATURE_X/Y 0.20` + `0.23` barrel scale). Disabled live when the **CURVE** toggle is off (`ControlState::curvature_enabled`, `crt_pi::CrtPiParams` zeroed, `barrel_inset_for_y` skipped).  
**Scanlines** — crt-pi weighted (`max(1−dist²·6, 0.12)`), multisampled, bloom-scaled, mask-modulated. Disabled when **SCAN** toggle off (skips `draw_scanlines` + `draw_aperture_mask`).  
**Vignette / corners** — edge gradient + corner darkening + radial vignette (radial component skipped when curvature off).  
**Bloom / glow** — text rendered twice (70-α + 35-α phosphor bloom halo) behind crisp phosphor colour, plus phosphor-tinted bezel spill.  
**Flicker / hum** — `sin(t·7.3)` + 60 Hz hum with fullscreen overlay; disabled when **FLICKER** toggle off (alpha fixed at 255, overlay skipped).

---

## Font — VT323

- Bundled at `assets/fonts/VT323-Regular.ttf` (150 KiB, SIL OFL 1.1, © Peter Hull).
- Also copied license at `assets/fonts/OFL.txt`.
- At runtime we try, in order: bundled paths (`assets/fonts/…`, exe-relative), then `/System/Library/Fonts/Monaco.ttf` etc. Missing font → graceful fallback to system mono.
- Point size auto-selected: we probe 28→14 pt and pick the largest that fits 80 cols + 24 rows inside the glass (`recommended_line_spacing`).

To fetch fresh at build time instead of bundling:

```bash
curl -L https://github.com/google/fonts/raw/main/ofl/vt323/VT323-Regular.ttf -o assets/fonts/VT323-Regular.ttf
```

---

## Architecture

```
.
├── Cargo.toml          — sdl2 (ttf) + rfd (file picker) + glow (crt-pi GL) + pure Rust Z-machine (vendored encrusted)
├── .cargo/config.toml  — adds /opt/homebrew/lib to linker + PKG_CONFIG_PATH on macOS
├── assets/
│   ├── fonts/VT323-Regular.ttf + OFL.txt
│   ├── shaders/crt-pi.glsl       — verbatim crt-pi shader (GPL-2.0+, © davej)
│   ├── shaders/crt-pi.vert/.frag — GL 3.3 core split for glow
│   ├── shaders/LICENSE.crt-pi    — source & license note
│   └── stories/                  — optional: place zork1.z3 here (not bundled)
└── src/
    ├── main.rs         — arg parsing, SDL/TTF init, GL attr, CrtGl probe, AppState (+ ControlState/mouse), main loop, mouse hit-test
    ├── cli.rs          — Cli + parse_args/print_help (now documents pure-Rust Z-machine, no dfrotz)
    ├── constants.rs    — COLS/ROWS/WINDOW_W/WINDOW_H/COLORS + safe conversions (PHOSPHOR now via ControlState)
    ├── controls.rs     — ControlState, PhosphorColor (Green/Amber/White), bottom-bar layout & hit-testing, crt_params()
    ├── grid.rs         — Grid (80×24, cursor, scroll, put_char)
    ├── font.rs         — font_search_paths, load_best_font, choose_font
    ├── backend.rs      — ZMachineSession (pure Rust), find_story, load_story_bytes, GuiUi
    ├── zmachine/       — vendored encrusted (MIT): buffer/frame/instruction/options/quetzal/traits/zmachine
    │   └── fixtures/minizork.z3 — test fixture
    ├── crt_pi.rs       — Rust port of crt-pi math (CalcScanLine, gamma, bloom, mask, distort)
    ├── crt_gl.rs       — glow wrapper: compiles crt-pi.vert/.frag, validates GL path
    └── render.rs       — CRT rendering: bezel (no screws/grille/plate), glass, phosphor-aware bloom text, crt-pi scanlines/mask/vignette, bottom controls (draw_bottom_controls/draw_bottom_control_labels)
```

- `find_story()` — CLI arg primary; else searches `./zork1.z3`, `./zork1.zip`, `./assets/stories/*`, `./stories/*`, ancestors and exe-relative paths. `.zip` archives are handled via the `zip` crate (first contained `.z*` story is extracted).
- `ZMachineSession::new(PathBuf)` — loads story bytes, validates version 1..=8, spawns a worker thread running `Zmachine::new(data, GuiUi, Options)` with 80×24 header (`set_screen_size(80,24)`), drives `step()`/`handle_input()` loop, streams output via `mpsc::Receiver<String>` and accepts input via `send_input(&str)`. Quit/Restore are handled internally; Save is no-op (reports success) — Quetzal persistence can be added later.
- `GuiUi` — implements `zmachine::traits::UI` with a channel sender; `print`/`print_object`/`clear` forward to the CRT grid, `message` surfaces save/restore notices, `set_status_bar` renders as a text line.
- main loop — SDL event pump (TextInput + KeyDown), non-blocking `poll_zmachine`, 60 Hz render: bezel → glass → bloom text → scanlines/vignette/flicker

**Why pure Rust vs dfrotz subprocess?**
Previously the app spawned `dfrotz -w 80 -h 24 -m -p` (GPL) via `Popen` and threads. The new backend vendors **encrusted** (MIT) — a pure-Rust Z-machine — so the binary is self-contained MIT, portable, and has no external `frotz` dependency. Research on crates.io shows no `zork-rs` crate exists; the closest MIT pure-Rust interpreters are `encrusted` (MIT, mature, supports .z3/.z5/.z8 via `Zmachine::step`/`handle_input`) and `ur-grue` (AGPL, stub/empty) and the empty `zvm-core` 0.100.0 WIP. `encrusted` was chosen for its MIT license, 80×24 dumb-terminal behaviour, and proven fixtures (`minizork.z3`, `czech.*`).

---

## Troubleshooting

- **`library 'SDL2_ttf' not found` at link** — you need `brew install sdl2_ttf` (macOS) or `libsdl2-ttf-dev` (Linux). The repo ships `.cargo/config.toml` that adds `/opt/homebrew/lib` to the linker; if you installed SDL elsewhere, set `LIBRARY_PATH` and `PKG_CONFIG_PATH` accordingly.
- **Blank/error screen** — you launched without a story and cancelled the picker. Use `cargo run -- --story /path/to/zork1.z3` or `cargo run -- --story assets/stories/zork1.z3` or press `F1` in-GUI.
- **Font looks wrong / not VT323** — ensure `assets/fonts/VT323-Regular.ttf` exists (it is bundled). Delete it to test fallback.
- **Window too big/small** — fixed at 1120×860 (bezel 48, pad 14, glass 1024×764). Resize is disabled (scales font instead). Edit `WINDOW_W/H` in `src/constants.rs` to re-tune.
- **`--story` says not found** — the resolver checks the given path plus ancestors and exe-relative locations. If you pass a relative path it is resolved against cwd ancestors; prefer an absolute path if unsure.
- **`version 0` or `unsupported Z-machine version`** — story file is missing, empty, or not a valid Z-code file. Ensure the file is a raw `.z3/.z5/.z8` (or a `.zip` containing one).

---

## Build verification (no display needed)

```bash
cargo check
cargo build
./target/debug/zork-crt-gui --help
./target/debug/zork-crt-gui --version
./target/debug/zork-crt-gui --story /nonexistent  # exits 1 with helpful message, no picker hang in CI
cargo clippy -- -D clippy::pedantic
cargo test
```

GUI smoke test (requires display + story):

```bash
cargo run -- --story /path/to/zork1.z3
cargo run -- --story src/zmachine/fixtures/minizork.z3
```

---

## License

- **Code** — MIT (see `LICENSE`). `src/zmachine/*` is vendored from [encrusted](https://github.com/demille/encrusted) (MIT) with local adaptations (80×24 header, `GuiUi` channel backend, `Send` trait, `paused_opcode`/`set_screen_size` helpers, zip support). `src/crt_pi.rs` (Rust port of the crt-pi algorithm) is also MIT.
- **crt-pi shader** — `assets/shaders/crt-pi.glsl` (+ `.vert`/`.frag` split) is **GPL-2.0+**, Copyright © 2015-2016 davej, from [libretro/glsl-shaders](https://github.com/libretro/glsl-shaders). See `assets/shaders/LICENSE.crt-pi`. Verbatim redistribution must comply with GPL-2.0+.
- **VT323 font** — SIL Open Font License 1.1 (`assets/fonts/OFL.txt`).
- **Zork I story** — original Infocom copyright if you provide one; not bundled in this repo. Use only stories you have rights to.
