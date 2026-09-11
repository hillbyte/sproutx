use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use evdev::raw_stream::RawDevice;
use evdev::{EventType, InputEvent, KeyCode};

use crate::capture;
use crate::config;
use crate::engine::{self, Engine};
use crate::injector::Injector;
use crate::ipc;
use crate::keys::{self, Planner, Reader};

enum Msg {
    Key(InputEvent),
}

struct Core {
    reader: Reader,
    planner: Planner,
    injector: Injector,
    buffer: Vec<char>,
    last_activity: Instant,
}

impl Core {
    fn handle(&mut self, ev: &InputEvent, engine: &Arc<Mutex<Engine>>) {
        if ev.event_type() != EventType::KEY {
            return;
        }
        self.last_activity = Instant::now();
        let code = ev.code();
        let value = ev.value();
        match value {
            0 => {
                self.reader.on_event(code, 0);
            }
            1 => {
                let text = self.reader.on_event(code, 1);
                if code == KeyCode::KEY_BACKSPACE.code() {
                    self.buffer.pop();
                    debug!("backspace -> buffer {:?}", self.buf_tail());
                    return;
                }
                if code == KeyCode::KEY_ENTER.code()
                    || code == KeyCode::KEY_KPENTER.code()
                    || code == KeyCode::KEY_ESC.code()
                {
                    let hit = {
                        let e = engine.lock().unwrap();
                        e.match_at_end(&self.buffer)
                            .map(|(trig, repl)| (trig, repl, e.delay_ms))
                    };
                    match hit {
                        Some((trig, repl, delay_ms)) => {
                            self.expand(trig, repl, Duration::from_millis(delay_ms.max(1)))
                        }
                        None => self.buffer.clear(),
                    }
                    return;
                }
                if keys::HOTKEY_MOD_KEYS.contains(&code) && !self.buffer.is_empty() {
                    debug!("cleared buffer on shortcut key");
                    self.buffer.clear();
                    return;
                }
                if let Some(text) = text {
                    let hit = {
                        let e = engine.lock().unwrap();
                        let mut hit = None;
                        for ch in text.chars() {
                            debug!("feed {ch:?} -> buffer {:?}", self.buf_tail());
                            if let Some((trig, repl)) = e.feed_char(&mut self.buffer, ch) {
                                hit = Some((trig, repl, e.delay_ms));
                                break;
                            }
                        }
                        hit
                    };
                    if let Some((trig, repl, delay_ms)) = hit {
                        self.expand(trig, repl, Duration::from_millis(delay_ms.max(1)));
                    }
                }
            }
            _ => {
                self.reader.on_event(code, value);
            }
        }
    }

    fn buf_tail(&self) -> String {
        self.buffer.iter().collect::<String>()
    }

    fn expand(&mut self, trig: String, repl: String, delay: Duration) {
        let tl = trig.chars().count();
        info!("expand {trig:?} -> {repl:?}");
        self.injector.backspace_n(tl, delay);
        std::thread::sleep(Duration::from_millis(40));
        let rendered = engine::render(&repl);
        self.injector.type_text(&self.planner, &rendered, delay);
        self.buffer.clear();
        self.last_activity = Instant::now();
    }
}

pub fn run(verbose: bool) -> Result<(), String> {
    crate::log::init(verbose);

    let cfg_path = config::default_path();
    if !cfg_path.exists() {
        error!("config not found at {}", cfg_path.display());
        error!("run `sproutx init` to create a sample config");
        return Err("no config file".into());
    }
    let cfg = config::load(&cfg_path).map_err(|e| format!("config: {e}"))?;
    let layout = config::resolve_layout(&cfg.layout);
    info!("layout: {layout}");
    let keymap = keys::compile(&layout)
        .ok_or_else(|| format!("cannot compile XKB keymap for layout '{layout}'"))?;
    let reader = Reader::new(&keymap);
    let planner = Planner::new(&keymap);

    let injector = Injector::create()
        .map_err(|e| format!("uinput init failed: {e} (is /dev/uinput writable?)"))?;
    info!(
        "virtual keyboard ready ({})",
        crate::injector::VDEV_NAME
    );

    let engine = Arc::new(Mutex::new(Engine::new(&cfg.rules, cfg.depth, cfg.delay_ms)));
    let running = Arc::new(AtomicBool::new(true));

    let sock = ipc::socket_path();
    if let Some(p) = sock.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::remove_file(&sock);
    let listener = std::os::unix::net::UnixListener::bind(&sock)
        .map_err(|e| format!("cannot bind {}: {e}", sock.display()))?;
    let _ = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o666));
    {
        let engine = engine.clone();
        let running = running.clone();
        let cfg_path = cfg_path.clone();
        let _ = std::thread::Builder::new()
            .name("sproutx-ipc".into())
            .spawn(move || {
                let _ = ipc::serve(listener, engine, running, cfg_path);
            });
    }
    info!("ipc ready on {}", sock.display());

    let (tx, rx) = mpsc::channel::<Msg>();
    let opened: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));

    spawn_open(&tx, &opened);
    let first_batch = opened.lock().unwrap().len();
    if first_batch == 0 {
        if capture::open_permission_denied() {
            error!("cannot read /dev/input/event* (permission denied).");
            error!("hint: add your user to the 'input' group and log in again:");
            error!("      sudo usermod -aG input $USER");
        } else {
            warn!("no keyboard devices detected");
        }
    }

    let mut core = Core {
        reader,
        planner,
        injector,
        buffer: Vec::new(),
        last_activity: Instant::now(),
    };

    let mut last_hotplug = Instant::now() - Duration::from_secs(4);
    loop {
        if !running.load(Ordering::Relaxed) {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(120)) {
            Ok(Msg::Key(ev)) => core.handle(&ev, &engine),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !running.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
        if !core.buffer.is_empty() && core.last_activity.elapsed() >= Duration::from_millis(2000) {
            debug!("cleared buffer on idle");
            core.buffer.clear();
        }
        if last_hotplug.elapsed() >= Duration::from_secs(3) {
            spawn_new(&tx, &opened);
            last_hotplug = Instant::now();
        }
    }

    let _ = std::fs::remove_file(&sock);
    info!("stopped");
    Ok(())
}

fn spawn_open(tx: &mpsc::Sender<Msg>, opened: &Arc<Mutex<HashSet<PathBuf>>>) {
    for (path, _dev) in capture::open_keyboards() {
        if opened.lock().unwrap().insert(path.clone()) {
            info!("found keyboard {}", path.display());
            spawn_reader(path.clone(), tx.clone(), opened.clone());
        }
    }
}

fn spawn_new(tx: &mpsc::Sender<Msg>, opened: &Arc<Mutex<HashSet<PathBuf>>>) {
    for (path, _dev) in capture::open_keyboards() {
        if opened.lock().unwrap().insert(path.clone()) {
            info!("found new keyboard {}", path.display());
            spawn_reader(path.clone(), tx.clone(), opened.clone());
        }
    }
}

fn spawn_reader(
    path: PathBuf,
    tx: mpsc::Sender<Msg>,
    opened: Arc<Mutex<HashSet<PathBuf>>>,
) {
    let _ = std::thread::Builder::new()
        .name("sproutx-read".into())
        .spawn(move || {
            let mut dev = match RawDevice::open(&path) {
                Ok(d) => d,
                Err(e) => {
                    debug!("cannot open {}: {e}", path.display());
                    return;
                }
            };
            info!("listening on {}", path.display());
            loop {
                match dev.fetch_events() {
                    Ok(evs) => {
                        for ev in evs {
                            if tx.send(Msg::Key(ev)).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        debug!("read error on {}: {e}", path.display());
                        break;
                    }
                }
            }
            opened.lock().unwrap().remove(&path);
        });
}