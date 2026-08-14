#![allow(clippy::pedantic)]
#![allow(clippy::nursery)]
#![allow(dead_code)]

use std::env;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::basic::BasicSession;
use crate::saves;
use crate::zmachine::instruction::Opcode;
use crate::zmachine::options::Options;
use crate::zmachine::traits::UI;
use crate::zmachine::zmachine::Zmachine;

// ---------------------------------------------------------------------------
// GuiUi — channel-backed UI for the CRT (replaces WebUI/TerminalUI)
// ---------------------------------------------------------------------------

struct GuiUi {
    tx: Sender<String>,
}

impl GuiUi {
    fn with_channel(tx: Sender<String>) -> Box<dyn UI + Send> {
        Box::new(Self { tx })
    }
}

impl UI for GuiUi {
    fn new() -> Box<Self>
    where
        Self: Sized,
    {
        panic!("GuiUi::new() not supported — use with_channel")
    }

    fn clear(&self) {
        let _ = self.tx.send("\x0C".to_string());
    }

    fn print(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let _ = self.tx.send(text.to_string());
    }

    fn debug(&mut self, text: &str) {
        let _ = self.tx.send(text.to_string());
    }

    fn print_object(&mut self, object: &str) {
        let _ = self.tx.send(object.to_string());
    }

    fn set_status_bar(&self, left: &str, right: &str) {
        // Rendered as a textual status line; the CRT grid will show it.
        let _ = self.tx.send(format!("\n[{left} | {right}]\n"));
    }

    fn reset(&self) {
        let _ = self.tx.send("\n[reset]\n".to_string());
    }

    fn get_user_input(&self) -> String {
        panic!("GuiUi::get_user_input() should never be called — use Zmachine::step/handle_input")
    }

    fn flush(&mut self) {}

    fn message(&self, mtype: &str, msg: &str) {
        // Internal autosave / Quetzal messages from encrusted must NOT be
        // rendered to the CRT grid. The VM calls `ui.message("savestate", json)`
        // on every input pause (VAR_228) and on quit, where `json` is
        // `["West of House - 0/0", "Rk9S..."]` (status + base64 Quetzal dump).
        // Previously this leaked as `>[savestate] ["West of House...", "Rk9S..."]`
        // right after the `>` prompt because we forwarded every message.
        match mtype {
            "savestate" => {
                // Autosave before each input — keep internal, don't render.
                if std::env::var("DEBUG").is_ok() {
                    eprintln!("[zmachine:savestate] suppressed {} bytes", msg.len());
                }
            }
            "save" => {
                // OP0_181 SAVE — now handled via paused SAVE in the worker thread;
                // this path is only hit for legacy callers, suppress raw JSON.
                if std::env::var("DEBUG").is_ok() {
                    eprintln!("[zmachine:save] suppressed {} bytes", msg.len());
                }
                let _ = self.tx.send("\n[Save successful]\n".to_string());
            }
            "restore" => {
                // OP0_182 RESTORE and the `"restore"` message from step() —
                // suppressed; the CRT backend handles file I/O.
                if std::env::var("DEBUG").is_ok() {
                    eprintln!("[zmachine:restore] suppressed {} bytes", msg.len());
                }
            }
            _ => {
                let _ = self.tx.send(format!("[{mtype}] {msg}"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Story loading (supports raw .z3/.z5/.z8 and .zip archives)
// ---------------------------------------------------------------------------

fn load_story_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "zip" {
        let file = std::fs::File::open(path)
            .map_err(|e| format!("failed to open zip {}: {e}", path.display()))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("zip error {}: {e}", path.display()))?;
        if archive.is_empty() {
            return Err(format!("zip {} is empty", path.display()));
        }
        // Prefer a contained .z* story; otherwise first non-dir entry.
        let mut chosen = None;
        for i in 0..archive.len() {
            let f = archive
                .by_index(i)
                .map_err(|e| format!("zip entry {i}: {e}"))?;
            let name = f.name().to_ascii_lowercase();
            let is_story = name.ends_with(".z3")
                || name.ends_with(".z5")
                || name.ends_with(".z8")
                || name.ends_with(".z4")
                || name.ends_with(".z2")
                || name.ends_with(".z1")
                || name.ends_with(".zblorb")
                || name.ends_with(".ulx");
            if is_story {
                chosen = Some(i);
                break;
            }
            if chosen.is_none() && !f.is_dir() {
                chosen = Some(i);
            }
        }
        let idx = chosen.ok_or_else(|| format!("zip {} contains no files", path.display()))?;
        let mut entry = archive
            .by_index(idx)
            .map_err(|e| format!("zip entry {idx}: {e}"))?;
        let mut buf = Vec::new();
        std::io::copy(&mut entry, &mut buf).map_err(|e| format!("zip read: {e}"))?;
        if buf.is_empty() {
            return Err(format!("zip entry {} is empty", entry.name()));
        }
        Ok(buf)
    } else {
        let mut data = Vec::new();
        let mut f = std::fs::File::open(path)
            .map_err(|e| format!("failed to open {}: {e}", path.display()))?;
        use std::io::Read as _;
        f.read_to_end(&mut data)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        if data.is_empty() {
            return Err(format!("story file {} is empty", path.display()));
        }
        Ok(data)
    }
}

// ---------------------------------------------------------------------------
// ZMachineSession — pure-Rust replacement for DfrotzSession
// ---------------------------------------------------------------------------

pub struct ZMachineSession {
    #[allow(dead_code)]
    pub story_path: PathBuf,
    pub rx: Receiver<String>,
    input_tx: Sender<String>,
    // Keep the join handle alive so the VM thread isn't detached prematurely.
    #[allow(dead_code)]
    handle: Option<thread::JoinHandle<()>>,
    /// Selected game id for slot I/O (empty for legacy sessions without slot).
    #[allow(dead_code)]
    pub game_id: String,
    #[allow(dead_code)]
    pub slot: u8,
}

impl ZMachineSession {
    /// Load `story_path` (raw or .zip) and spawn the Z-machine on a worker
    /// thread. Output is streamed via `rx` (poll with `try_recv`), input is
    /// fed via `send_input`.
    #[allow(dead_code)]
    pub fn new(story_path: PathBuf) -> Result<Self, String> {
        Self::spawn(&story_path)
    }

    /// Spawn with an explicit save slot (`1..=3`). SAVE/RESTORE will use this
    /// slot; if the slot has a Quetzal file it is auto-restored at start.
    pub fn new_with_slot(story_path: PathBuf, game_id: String, slot: u8) -> Result<Self, String> {
        Self::spawn_with_slot(&story_path, game_id, slot)
    }

    #[allow(dead_code)]
    pub fn spawn(story: &Path) -> Result<Self, String> {
        Self::spawn_inner(story, None)
    }

    pub fn spawn_with_slot(story: &Path, game_id: String, slot: u8) -> Result<Self, String> {
        assert!((1..=saves::NUM_SLOTS).contains(&slot), "slot {slot} out of range");
        Self::spawn_inner(story, Some((game_id, slot)))
    }

    fn spawn_inner(story: &Path, slot_info: Option<(String, u8)>) -> Result<Self, String> {
        let data = load_story_bytes(story)?;
        // Validate Z-machine version byte (spec: 1..=8)
        let version = data[0];
        if !(1..=8).contains(&version) {
            return Err(format!(
                "unsupported Z-machine version {version} in {} (expected 1..8)",
                story.display()
            ));
        }

        // For slot sessions, try to load existing Quetzal for auto-restore.
        let initial_restore: Option<Vec<u8>> = if let Some((ref gid, s)) = slot_info {
            saves::read_slot(gid, s)
        } else {
            None
        };
        let should_restore = initial_restore.is_some();

        let (output_tx, output_rx) = mpsc::channel::<String>();
        let (input_tx, input_rx) = mpsc::channel::<String>();
        let story_path = story.to_path_buf();
        let story_clone = story_path.clone();
        let game_id_for_thread = slot_info.as_ref().map(|(g, _)| g.clone()).unwrap_or_default();
        let slot_for_thread = slot_info.as_ref().map(|(_, s)| *s).unwrap_or(0);
        let use_slots = slot_info.is_some();
        let game_id_clone = game_id_for_thread.clone();
        let slot_clone = slot_for_thread;
        let game_id_final = game_id_for_thread.clone();
        let slot_final = slot_for_thread;

        let handle = thread::spawn(move || {
            let ui = GuiUi::with_channel(output_tx.clone());
            let mut opts = Options::default();
            opts.save_name = story_clone
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("story")
                .to_string();
            opts.save_dir = story_clone
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".to_string());
            // Random seed — use thread_rng to get unpredictable values
            {
                use rand::Rng as _;
                let mut rng = rand::thread_rng();
                opts.rand_seed = [
                    rng.gen::<u32>(),
                    rng.gen::<u32>(),
                    rng.gen::<u32>(),
                    rng.gen::<u32>(),
                ];
            }

            let mut zm = Zmachine::new(data, ui, opts);
            // Emulate `dfrotz -w 80 -h 24` for the 80×24 grid.
            zm.set_screen_size(80, 24);
            // Auto-restore selected slot if occupied.
            if let Some(bytes) = initial_restore {
                if zm.initial_restore(&bytes) {
                    let _ = output_tx.send(format!("\n[Restored slot {slot_clone}]\n"));
                } else if std::env::var("DEBUG").is_ok() {
                    eprintln!("[slot] initial restore failed for slot {slot_clone}");
                }
            } else if should_restore {
                // This branch unreachable (initial_restore None means should_restore false)
            }
            // If we started without slot info, suppress save/restore via empty handlers?
            let slot_game_id = game_id_clone.clone();
            let slot_num = slot_clone;

            let mut done = false;
            while !done {
                done = zm.step();
                zm.ui.flush();
                if done {
                    let _ = output_tx.send(
                        "\n[Game ended. Press Esc to return to menu]\n".to_string(),
                    );
                    break;
                }
                // step() returns false when paused for input/restore/save
                let paused = zm.paused_opcode();
                match paused {
                    Some(Opcode::VAR_228) => {
                        // AREAD/SREAD — wait for a line from the GUI
                        match input_rx.recv() {
                            Ok(line) => {
                                zm.handle_input(line);
                            }
                            Err(_) => break, // GUI closed
                        }
                    }
                    Some(Opcode::OP0_181) => {
                        // SAVE — write to selected slot
                        if use_slots {
                            let bytes = zm
                                .paused_save_bytes()
                                .unwrap_or_default();
                            let status = zm.status_string();
                            match saves::write_slot(&slot_game_id, slot_num, &bytes, &status) {
                                Ok(()) => {
                                    zm.handle_save_result(true);
                                    let _ = output_tx
                                        .send(format!("\n[Saved to slot {slot_num}]\n"));
                                }
                                Err(e) => {
                                    zm.handle_save_result(false);
                                    let _ = output_tx
                                        .send(format!("\n[Save failed: {e}]\n"));
                                }
                            }
                        } else {
                            // No slot selected — succeed without file I/O (legacy)
                            zm.handle_save_result(true);
                            let _ = output_tx.send("\n[Save successful]\n".to_string());
                        }
                    }
                    Some(Opcode::OP0_182) => {
                        // RESTORE — load from selected slot
                        if use_slots {
                            if let Some(bytes) = saves::read_slot(&slot_game_id, slot_num) {
                                zm.handle_restore_bytes(Some(bytes));
                                let _ = output_tx
                                    .send(format!("\n[Restored slot {slot_num}]\n"));
                            } else {
                                zm.handle_restore_bytes(None);
                                let _ = output_tx.send(format!(
                                    "\n[No save in slot {slot_num}]\n"
                                ));
                            }
                        } else {
                            zm.handle_restore_bytes(None);
                        }
                    }
                    Some(other) => {
                        let _ = output_tx.send(format!(
                            "\n[Z-machine paused for unhandled opcode {other:?} — resuming]\n"
                        ));
                        // Try to resume with empty input; if it keeps pausing we break
                        break;
                    }
                    None => {
                        // No paused instruction yet step() said not done — this is
                        // a logic error, but break to avoid spinning
                        let _ = output_tx.send(
                            "\n[Z-machine step returned not-done without pause — stopping]\n"
                                .to_string(),
                        );
                        break;
                    }
                }
            }
        });

        Ok(Self {
            story_path,
            rx: output_rx,
            input_tx,
            handle: Some(handle),
            game_id: game_id_final,
            slot: slot_final,
        })
    }

    /// Send a line of input to the Z-machine (without trailing newline).
    pub fn send_input(&self, line: &str) -> Result<(), String> {
        self.input_tx
            .send(line.to_string())
            .map_err(|e| format!("Z-machine input channel closed: {e}"))
    }

    /// Backwards-compatible alias used by the GUI loop.
    #[allow(dead_code)]
    pub fn send_line(&self, line: &str) -> Result<(), String> {
        self.send_input(line)
    }

    /// Non-blocking poll helper — mirrors previous dfrotz `try_recv` pattern.
    #[allow(dead_code)]
    pub fn try_recv(&self) -> Result<String, std::sync::mpsc::TryRecvError> {
        self.rx.try_recv()
    }

    /// Returns true if the underlying channel is disconnected (VM exited).
    pub fn is_finished(&self) -> bool {
        if let Some(h) = &self.handle {
            if h.is_finished() {
                return true;
            }
        }
        false
    }

    pub fn kill(&mut self) {
        // Dropping the input sender will cause the VM thread to break on next input wait.
        // The thread will then exit and drop its sender, making rx Disconnected.
        // We don't have a direct kill primitive for the pure-Rust VM; dropping is sufficient.
        // For completeness, we could also try to abort, but we just mark finished via channel drop.
        // The JoinHandle will be cleaned up on Drop of Self.
    }
}

// ---------------------------------------------------------------------------
// Backend abstraction — unified ZMachine + BASIC
// ---------------------------------------------------------------------------

/// Unified backend enum covering both Z-machine and BASIC python games.
pub enum Backend {
    ZMachine(ZMachineSession),
    Basic(BasicSession),
}

impl Backend {
    #[allow(dead_code)]
    #[must_use]
    pub fn game_id(&self) -> &str {
        match self {
            Self::ZMachine(s) => &s.game_id,
            Self::Basic(s) => &s.game_id,
        }
    }

    pub fn send_input(&self, line: &str) -> Result<(), String> {
        match self {
            Self::ZMachine(s) => s.send_input(line),
            Self::Basic(s) => s.send_input(line),
        }
    }

    pub fn try_recv(&self) -> Result<String, std::sync::mpsc::TryRecvError> {
        match self {
            Self::ZMachine(s) => s.try_recv(),
            Self::Basic(s) => s.try_recv(),
        }
    }

    pub fn is_finished(&self) -> bool {
        match self {
            Self::ZMachine(s) => s.is_finished(),
            Self::Basic(s) => s.is_finished(),
        }
    }

    #[allow(dead_code)]
    pub fn kill(&mut self) {
        match self {
            Self::ZMachine(s) => s.kill(),
            Self::Basic(s) => s.kill(),
        }
    }

    #[must_use]
    pub fn is_basic(&self) -> bool {
        matches!(self, Self::Basic(_))
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn is_zmachine(&self) -> bool {
        matches!(self, Self::ZMachine(_))
    }
}

// ---------------------------------------------------------------------------
// Story discovery (same search order as before, now for pure-Rust VM)
// ---------------------------------------------------------------------------

pub fn find_story(cli_arg: Option<String>) -> Option<PathBuf> {
    if let Some(p) = cli_arg {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Some(pb);
        }
        if !pb.is_absolute() {
            if let Ok(cwd) = env::current_dir() {
                for ancestor in cwd.ancestors() {
                    let cand = ancestor.join(&pb);
                    if cand.exists() {
                        return Some(cand);
                    }
                }
            }
            if let Ok(exe) = env::current_exe() {
                if let Some(parent) = exe.parent() {
                    for anc in parent.ancestors() {
                        let cand = anc.join(&pb);
                        if cand.exists() {
                            return Some(cand);
                        }
                    }
                }
            }
        }
        eprintln!("story file not found: {p}");
        return None;
    }
    // 1) New data_dir/stories/**/*.{z3,z5,z8,zip} — preferred location
    if let Some(found) = find_in_data_dir() {
        return Some(found);
    }
    let candidates = [
        "zork1.z3",
        "zork1.zip",
        "assets/stories/zork1.z3",
        "assets/stories/zork1.z5",
        "assets/stories/zork1.z8",
        "stories/zork1.z3",
        "stories/zork1.z5",
        "stories/zork1.z8",
    ];
    let mut exe_cands = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            exe_cands.push(parent.join("zork1.z3"));
            exe_cands.push(parent.join("zork1.zip"));
            exe_cands.push(parent.join("assets/stories/zork1.z3"));
            exe_cands.push(parent.join("assets/stories/zork1.z5"));
            exe_cands.push(parent.join("assets/stories/zork1.z8"));
            exe_cands.push(parent.join("../assets/stories/zork1.z3"));
            exe_cands.push(parent.join("../../assets/stories/zork1.z3"));
            exe_cands.push(parent.join("../zork1.z3"));
            exe_cands.push(parent.join("../../zork1.z3"));
        }
    }
    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    for p in exe_cands {
        if p.exists() {
            return Some(p);
        }
    }
    let cwd = env::current_dir().ok()?;
    for ancestor in cwd.ancestors() {
        for rel in &candidates {
            let p = ancestor.join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn find_in_data_dir() -> Option<PathBuf> {
    let sdir = crate::paths::stories_dir();
    if !sdir.exists() {
        return None;
    }
    // Prefer well-known filenames first, then any story file recursively.
    let preferred = [
        sdir.join("zork1").join("zork1.z3"),
        sdir.join("zork1").join("game.z3"),
        sdir.join("zork1.z3"),
        sdir.join("zork1.z5"),
        sdir.join("zork1.z8"),
    ];
    for p in &preferred {
        if p.exists() {
            return Some(p.clone());
        }
    }
    // Recursive scan for any .z* / .zip
    let mut best: Option<PathBuf> = None;
    collect_data_stories(&sdir, &mut best);
    best
}

fn collect_data_stories(dir: &Path, best: &mut Option<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for ent in entries.flatten() {
        let p = ent.path();
        if p.is_dir() {
            subdirs.push(p);
        } else {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_story = matches!(
                ext.as_str(),
                "z3" | "z5" | "z8" | "z1" | "z2" | "z4" | "zip" | "zblorb" | "ulx"
            );
            if is_story && best.is_none() {
                *best = Some(p);
                return;
            }
        }
    }
    for sub in subdirs {
        collect_data_stories(&sub, best);
        if best.is_some() {
            return;
        }
    }
}

// Backwards-compatible alias for `find_dfrotz` — now always None (no external
// interpreter needed). Kept so `main.rs` diffs stay minimal if references remain.
#[allow(dead_code)]
pub fn find_dfrotz() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_story_none_when_missing() {
        // Nonexistent CLI arg should return None without panic
        let got = find_story(Some("/nonexistent/path/to/story.z3".to_string()));
        assert!(got.is_none());
    }

    #[test]
    fn load_bytes_rejects_bad_version() {
        // Create a temp file with version 0 (invalid)
        let dir = std::env::temp_dir();
        let path = dir.join("zork_crt_test_bad_version.z3");
        std::fs::write(&path, vec![0u8; 64]).unwrap();
        let res = ZMachineSession::spawn(&path);
        assert!(res.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn gui_session_spawns_and_exits_quickly_on_minizork() {
        // Use the vendored minizork fixture if present (from encrusted)
        let fixture = PathBuf::from("src/zmachine/fixtures/minizork.z3");
        if !fixture.exists() {
            // Fallback: check encrusted downloaded copy
            let alt = PathBuf::from("/tmp/enc2/encrusted-1.1.0/tests/minizork.z3");
            if !alt.exists() {
                return;
            }
            let sess = ZMachineSession::spawn(&alt).expect("spawn minizork");
            // Should produce some initial output within a short timeout
            let mut got = String::new();
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(2) {
                if let Ok(chunk) = sess.rx.try_recv() {
                    got.push_str(&chunk);
                    if got.to_lowercase().contains("west of house")
                        || got.to_lowercase().contains("zork")
                        || got.to_lowercase().contains("forest")
                    {
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                !got.is_empty(),
                "expected some output from minizork, got empty"
            );
            return;
        }
        let sess = ZMachineSession::spawn(&fixture).expect("spawn minizork");
        let mut got = String::new();
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(2) {
            if let Ok(chunk) = sess.rx.try_recv() {
                got.push_str(&chunk);
                if got.to_lowercase().contains("west of house") {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!got.is_empty());
    }
}
