# PLAY-CRT

Z-machine + 95 BASIC games in a phosphor CRT — 80×24 VT323 with shader and bottom switches.

Runs any `.z3/.z5/.z8/.zip` via a pure-Rust Z-machine (vendored [encrusted], MIT) and 95 Python BASIC games (from [basic-computer-games](https://github.com/coding-horror/basic-computer-games)). CRT look: PUBLIC DOMAIN CRT shader (`assets/shaders/crt-lottes.vert` / `.frag` — crt-lottes by Timothy Lottes, PUBLIC DOMAIN, vendored as `crt-lottes.glsl`, plus `crt-lottes.vert`/`frag` GL3.3 split — curvature via warpX/warpY, scanlines, bloom, mask), dark plastic bezel, and an interactive bottom control strip (phosphor colour + toggles). CPU fallback path (`src/crt_pi.rs` + SDL2) remains MIT.

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

## Attributions

All third-party works are fetched or vendored with their original licenses. Nothing is bundled beyond the manifests — stories and BASIC games download on demand.

| Component | Source | Author / License |
|-----------|--------|------------------|
| **Zork I/II/III, Planetfall, Mini-Zork** | [historicalsource/zork1](https://github.com/historicalsource/zork1) · [zork2](https://github.com/historicalsource/zork2) · [zork3](https://github.com/historicalsource/zork3) · [planetfall](https://github.com/historicalsource/planetfall) · [minizork-1987](https://github.com/historicalsource/minizork-1987) · mirrored at [IfArchive](https://ifarchive.org) | Original © 1980–1982 [Infocom, Inc.](https://en.wikipedia.org/wiki/Infocom) — distributed by [historicalsource](https://github.com/historicalsource) for non-commercial preservation. Not bundled; downloaded via `assets/manifests/stories.json` (`© Infocom — historical source, non-commercial`). |
| **BASIC Computer Games** (95 Python ports) | [coding-horror/basic-computer-games](https://github.com/coding-horror/basic-computer-games) | Book by [David Ahl](https://en.wikipedia.org/wiki/BASIC_Computer_Games) (1973/1978); modern Python ports by Jeff Atwood and [contributors](https://github.com/coding-horror/basic-computer-games/graphs/contributors) (~13k★). [Unlicense](https://unlicense.org) (public-domain dedication). Fetched via `assets/manifests/basic.json`. |
| **Z-machine** (`src/zmachine/`) | [DeMille/encrusted](https://github.com/DeMille/encrusted) (crate `encrusted` 1.1) | By Sterling DeMille — [MIT](https://github.com/DeMille/encrusted/blob/master/LICENSE) (© 2018). Vendored and extended (Quetzal, `.zip`, 80×24). |
| **CRT shader** (`assets/shaders/crt-lottes.*`) | [libretro/glsl-shaders — crt-lottes.glsl](https://github.com/libretro/glsl-shaders/blob/master/crt/shaders/crt-lottes.glsl) ([raw](https://raw.githubusercontent.com/libretro/glsl-shaders/master/crt/shaders/crt-lottes.glsl)) | *crt-lottes* by [Timothy Lottes](https://timothylottes.github.io/) — **PUBLIC DOMAIN** (“Please take and use, change, or whatever.” — see `assets/shaders/LICENSE` and header in `crt-lottes.glsl` / `.vert` / `.frag`). GL 3.3 split is a mechanical port. |
| **Font VT323** (`assets/fonts/VT323-Regular.ttf`) | [Google Fonts — VT323](https://fonts.google.com/specimen/VT323) | By Peter Hull (peter.hull@oikoi.com) — [SIL OFL 1.1](http://scripts.sil.org/OFL) (`assets/fonts/OFL.txt`). |
| **SDL2 + Rust bindings** | [libsdl.org](https://libsdl.org) · [Rust-SDL2/rust-sdl2](https://github.com/Rust-SDL2/rust-sdl2) · [glow](https://github.com/grovesNL/glow) | SDL2 is zlib-licensed; `rust-sdl2` is MIT, `glow` is MIT/Apache-2.0, `sdl2_ttf` via SDL_ttf. CPU fallback `src/crt_pi.rs` is clean-room MIT © 2026 batk0 (not derived from crt-lottes). |

> If you redistribute PLAY-CRT, keep the license files in `assets/shaders/LICENSE` and `assets/fonts/OFL.txt` and respect the Infocom non-commercial preservation terms.

---

## License

- **Code** — MIT (see `LICENSE`, © 2026 batk0). `src/zmachine/*` remains MIT (encrusted). `src/crt_pi.rs` + SDL2 fallback is MIT. CRT shader files are PUBLIC DOMAIN (see above).
- **VT323 font** — SIL Open Font License 1.1 (`assets/fonts/OFL.txt`).
- **Game content** — not bundled. Infocom titles © Infocom (non-commercial preservation only); BASIC games Unlicense. Use only content you have rights to.
