use std::env;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

// Fields dfrotz_path/story_path kept for debugging/introspection; allow dead_code to keep warnings minimal.
#[allow(dead_code)]
pub struct DfrotzSession {
    pub child: Child,
    pub stdin: ChildStdin,
    pub rx: Receiver<String>,
    pub dfrotz_path: PathBuf,
    pub story_path: PathBuf,
}

pub fn find_dfrotz() -> Option<PathBuf> {
    let cands = [
        "/opt/homebrew/bin/dfrotz",
        "/usr/local/bin/dfrotz",
        "/usr/bin/dfrotz",
    ];
    for p in cands {
        if Path::new(p).exists() {
            return Some(PathBuf::from(p));
        }
    }
    if let Ok(out) = Command::new("which").arg("dfrotz").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() && Path::new(&s).exists() {
                return Some(PathBuf::from(s));
            }
        }
    }
    for name in ["dfrotz", "frotz"] {
        if let Ok(out) = Command::new("which").arg(name).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(PathBuf::from(s));
                }
            }
        }
    }
    None
}

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

pub fn spawn_dfrotz(dfrotz: &Path, story: &Path) -> Result<DfrotzSession, String> {
    let mut child = Command::new(dfrotz)
        .arg("-w")
        .arg("80")
        .arg("-h")
        .arg("24")
        .arg("-m")
        .arg("-p")
        .arg(story)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn dfrotz ({}): {}", dfrotz.display(), e))?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let stdin = child.stdin.take().ok_or("no stdin")?;

    let (tx, rx) = mpsc::channel::<String>();
    let tx2 = tx.clone();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = [0u8; 2048];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = tx2.send(s);
                }
            }
        }
    });
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buf = [0u8; 2048];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = tx.send(format!("[stderr] {s}"));
                }
            }
        }
    });

    Ok(DfrotzSession {
        child,
        stdin,
        rx,
        dfrotz_path: dfrotz.to_path_buf(),
        story_path: story.to_path_buf(),
    })
}
