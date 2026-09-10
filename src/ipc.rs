use std::io::{BufRead, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::engine::Engine;

pub fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid())));
    base.join("sproutx.sock")
}

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

pub fn send(cmd: &str) -> Result<String, String> {
    let path = socket_path();
    let mut s = UnixStream::connect(&path).map_err(|e| {
        format!(
            "cannot connect to daemon at {}: {e} (is it running?)",
            path.display()
        )
    })?;
    s.write_all(cmd.as_bytes())
        .and_then(|_| s.write_all(b"\n"))
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

pub fn serve(
    listener: UnixListener,
    engine: Arc<Mutex<Engine>>,
    running: Arc<AtomicBool>,
    cfg_path: PathBuf,
) -> std::io::Result<()> {
    for conn in listener.incoming() {
        let mut s = match conn {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut line = String::new();
        let nread = {
            let mut r = std::io::BufReader::new(&s);
            r.read_line(&mut line).unwrap_or(0)
        };
        if nread == 0 {
            continue;
        }
        let cmd = line.trim().to_string();
        let reply = match cmd.as_str() {
            "ping" => "ok sproutx".to_string(),
            "status" => format!("ok running rules={}", engine.lock().unwrap().rules().len()),
            "reload" => match crate::config::load(&cfg_path) {
                Ok(cfg) => {
                    let layout = crate::config::resolve_layout(&cfg.layout);
                    let n = {
                        let mut eng = engine.lock().unwrap();
                        *eng = crate::engine::Engine::new(&cfg.rules, cfg.depth);
                        eng.rules().len()
                    };
                    crate::info!("config reloaded from {}", cfg_path.display());
                    format!("ok layout={layout} rules={n}")
                }
                Err(e) => format!("err: {e}"),
            },
            "list" => {
                let eng = engine.lock().unwrap();
                let mut out = String::from("ok");
                for (t, r) in eng.rules() {
                    let preview: String = r.chars().take(60).collect();
                    let suffix = if preview.len() != r.len() { "..." } else { "" };
                    out.push_str(&format!("\n  {t:?} => {preview}{suffix}"));
                }
                out
            }
            "stop" => {
                running.store(false, Ordering::Relaxed);
                "ok stopped".to_string()
            }
            other => format!("err: unknown command {other}"),
        };
        let _ = s.write_all(reply.as_bytes());
        let _ = s.write_all(b"\n");
    }
    Ok(())
}