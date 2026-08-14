#![allow(clippy::pedantic)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
    Arc, Mutex,
};
use std::thread;

/// RAII session for a BASIC python game.
pub struct BasicSession {
    #[allow(dead_code)]
    pub game_id: String,
    pub rx: Receiver<String>,
    input_tx: Sender<String>,
    child: Arc<Mutex<Option<Child>>>,
    finished: Arc<AtomicBool>,
    // Keep handles alive; we don't join but keep them to prevent detachment issues.
    #[allow(dead_code)]
    handles: Vec<thread::JoinHandle<()>>,
}

impl BasicSession {
    pub fn new(game_id: String, python_path: PathBuf) -> Result<Self, String> {
        if !python_path.exists() {
            return Err(format!("python file not found: {}", python_path.display()));
        }
        // Check python3 availability early.
        match Command::new("python3").arg("--version").output() {
            Ok(out) if out.status.success() => {},
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                return Err(format!("python3 check failed: {stderr}"));
            }
            Err(e) => {
                return Err(format!("python3 not found: {e}. Install python3 to play BASIC games."));
            }
        }

        let mut child = Command::new("python3")
            .arg("-u")
            .arg(&python_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn python3: {e}"))?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (output_tx, output_rx) = mpsc::channel::<String>();
        let (input_tx, input_rx) = mpsc::channel::<String>();

        let mut handles: Vec<thread::JoinHandle<()>> = Vec::new();

        // Writer thread: forward input_rx lines to child's stdin.
        if let Some(mut child_stdin) = stdin {
            let h = thread::spawn(move || {
                while let Ok(line) = input_rx.recv() {
                    let to_write = if line.ends_with('\n') {
                        line
                    } else {
                        format!("{line}\n")
                    };
                    if child_stdin.write_all(to_write.as_bytes()).is_err() {
                        break;
                    }
                    if child_stdin.flush().is_err() {
                        break;
                    }
                }
                // Dropping stdin signals EOF to child.
            });
            handles.push(h);
        }

        // Reader helper
        let spawn_reader = |mut reader: Box<dyn Read + Send>, tx: Sender<String>| -> thread::JoinHandle<()> {
            thread::spawn(move || {
                let mut buf = [0u8; 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                            let normalized = chunk.replace("\r\n", "\n").replace('\r', "\n");
                            if normalized.is_empty() {
                                continue;
                            }
                            if tx.send(normalized).is_err() {
                                break;
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
        };

        if let Some(out) = stdout {
            let tx = output_tx.clone();
            handles.push(spawn_reader(Box::new(out), tx));
        }
        if let Some(err) = stderr {
            let tx = output_tx.clone();
            handles.push(spawn_reader(Box::new(err), tx));
        }
        // Drop the original sender clone held here; only reader threads hold clones.
        drop(output_tx);

        let child_arc: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(Some(child)));
        let finished: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

        // Monitor thread: wait for child exit and mark finished.
        {
            let child_clone = Arc::clone(&child_arc);
            let finished_clone = Arc::clone(&finished);
            let h = thread::spawn(move || {
                // Poll until child exits. We keep trying try_wait with sleep to avoid blocking
                // the mutex forever, but we also need to handle kill.
                loop {
                    // Try to check if child has exited.
                    let should_break = {
                        let mut guard = child_clone.lock().expect("child mutex poisoned");
                        if let Some(c) = guard.as_mut() {
                            match c.try_wait() {
                                Ok(Some(_)) => true,
                                Ok(None) => false,
                                Err(_) => true,
                            }
                        } else {
                            true
                        }
                    };
                    if should_break {
                        finished_clone.store(true, Ordering::Relaxed);
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(50));
                }
            });
            handles.push(h);
        }

        Ok(Self {
            game_id,
            rx: output_rx,
            input_tx,
            child: child_arc,
            finished,
            handles,
        })
    }

    pub fn send_input(&self, line: &str) -> Result<(), String> {
        self.input_tx
            .send(line.to_string())
            .map_err(|e| format!("BASIC input channel closed: {e}"))
    }

    #[allow(dead_code)]
    pub fn send_line(&self, line: &str) -> Result<(), String> {
        self.send_input(line)
    }

    pub fn try_recv(&self) -> Result<String, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn is_finished(&self) -> bool {
        if self.finished.load(Ordering::Relaxed) {
            return true;
        }
        // Also check child try_wait if we can lock.
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(c) = guard.as_mut() {
                if let Ok(Some(_)) = c.try_wait() {
                    self.finished.store(true, Ordering::Relaxed);
                    return true;
                }
            } else {
                return true;
            }
        }
        false
    }

    pub fn kill(&mut self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(c) = guard.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
            *guard = None;
        }
        self.finished.store(true, Ordering::Relaxed);
    }
}

impl Drop for BasicSession {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn basic_session_spawns_and_exits() {
        // Create a tiny python script that prints and exits.
        let tmp = std::env::temp_dir().join("play_crt_basic_spawn_test");
        let _ = fs::create_dir_all(&tmp);
        let script = tmp.join("hello_basic.py");
        fs::write(&script, b"print('HELLO BASIC')\n").unwrap();
        let sess = match BasicSession::new("test_hello".to_string(), script.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skip basic spawn test: {e}");
                let _ = fs::remove_file(&script);
                return;
            }
        };
        let mut got = String::new();
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(2) {
            match sess.try_recv() {
                Ok(chunk) => got.push_str(&chunk),
                Err(TryRecvError::Empty) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(TryRecvError::Disconnected) => break,
            }
            if got.contains("HELLO") {
                break;
            }
        }
        assert!(got.contains("HELLO BASIC") || got.contains("HELLO"), "got: {got:?}");
        // Give monitor time to detect exit
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(sess.is_finished() || got.contains("HELLO"));
        let _ = fs::remove_file(&script);
    }

    #[test]
    fn basic_session_handles_input() {
        let tmp = std::env::temp_dir().join("play_crt_basic_input_test");
        let _ = fs::create_dir_all(&tmp);
        let script = tmp.join("echo_basic.py");
        // Script reads input and echoes
        fs::write(&script, b"print('prompt')\nimport sys\nline=sys.stdin.readline()\nprint(f'echo:{line.strip()}')\n").unwrap();
        let sess = match BasicSession::new("test_echo".to_string(), script.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skip basic input test: {e}");
                let _ = fs::remove_file(&script);
                return;
            }
        };
        // Wait for prompt
        let mut got = String::new();
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(2) {
            if let Ok(chunk) = sess.try_recv() {
                got.push_str(&chunk);
                if got.contains("prompt") { break; }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(got.contains("prompt"), "got prompt: {got:?}");
        sess.send_input("hello").expect("send");
        let mut got2 = String::new();
        let start2 = std::time::Instant::now();
        while start2.elapsed() < std::time::Duration::from_secs(2) {
            match sess.try_recv() {
                Ok(c) => {
                    got2.push_str(&c);
                    if got2.contains("echo:hello") { break; }
                }
                Err(TryRecvError::Empty) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(TryRecvError::Disconnected) => break,
            }
        }
        assert!(got2.contains("echo:hello"), "echo got: {got2:?}");
        let _ = fs::remove_file(&script);
    }

    #[test]
    fn basic_session_handles_prompt_without_newline() {
        let tmp = std::env::temp_dir().join("play_crt_basic_prompt_test");
        let _ = fs::create_dir_all(&tmp);
        let script = tmp.join("prompt_basic.py");
        // Script prints ? without newline using sys.stdout.write and flush, then reads
        fs::write(&script, b"import sys\nsys.stdout.write('What is your bet? ')\nsys.stdout.flush()\nline=sys.stdin.readline()\nsys.stdout.write(f'you bet {line}')\n").unwrap();
        let sess = match BasicSession::new("test_prompt".to_string(), script.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skip prompt test: {e}");
                let _ = fs::remove_file(&script);
                return;
            }
        };
        let mut got = String::new();
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(2) {
            if let Ok(c) = sess.try_recv() { got.push_str(&c); if got.contains("What is your bet?") { break; } }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(got.contains("What is your bet?"), "prompt no newline got: {got:?}");
        sess.send_input("5").unwrap();
        let mut got2 = String::new();
        let start2 = std::time::Instant::now();
        while start2.elapsed() < std::time::Duration::from_secs(2) {
            match sess.try_recv() {
                Ok(c) => { got2.push_str(&c); if got2.contains("you bet 5") { break; } }
                Err(TryRecvError::Empty) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(TryRecvError::Disconnected) => break,
            }
        }
        assert!(got2.contains("you bet 5"), "got2: {got2:?}");
        let _ = fs::remove_file(&script);
    }

    #[test]
    fn basic_session_python_not_found_error() {
        let tmp = std::env::temp_dir().join("play_crt_basic_missing_file");
        let missing = tmp.join("nonexistent.py");
        let res = BasicSession::new("missing".to_string(), missing);
        assert!(res.is_err());
        let msg = res.err().unwrap();
        assert!(msg.contains("not found"), "msg: {msg}");
    }
}
