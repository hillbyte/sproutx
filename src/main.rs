#[macro_use]
mod log;

mod capture;
mod config;
mod daemon;
mod engine;
mod injector;
mod ipc;
mod keys;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage(0);
    }
    let has = |flag: &str| args[1..].iter().any(|a| a == flag);

    match args[0].as_str() {
        "daemon" => run_daemon(has("-v") || has("--verbose")),
        "init" => cmd_init(has("--force")),
        "inspect" => cmd_inspect(),
        "list" => cmd_list(),
        "reload" => cmd_ipc("reload"),
        "status" => cmd_status(),
        "stop" => cmd_ipc("stop"),
        "add" => {
            if args.len() < 3 {
                eprintln!("usage: sproutx add <trigger> <replacement>");
                std::process::exit(2);
            }
            cmd_add(&args[1], &args[2..].join(" "))
        }
        "test" => {
            if args.len() < 2 {
                eprintln!("usage: sproutx test <text>");
                std::process::exit(2);
            }
            cmd_test(&args[1])
        }
        "install" => cmd_install(),
        "uninstall" => cmd_uninstall(args[1..].iter().any(|a| a == "--keep-config")),
        "help" | "--help" | "-h" => usage(0),
        other => {
            eprintln!("unknown command: {other}");
            usage(2);
        }
    }
}

fn usage(code: i32) -> ! {
    eprintln!(
        "sproutx {} - system-wide text expander (evdev + uinput, no GUI)\n\n\
         USAGE:\n\
         \x20 sproutx daemon [-v]        run the expander daemon\n\
         \x20 sproutx init [--force]     write a sample config\n\
         \x20 sproutx inspect            list input devices and permissions\n\
         \x20 sproutx list               show active rules (daemon, or config file)\n\
         \x20 sproutx reload             tell the daemon to reload its config\n\
         \x20 sproutx status             is the daemon running?\n\
         \x20 sproutx stop               stop the daemon\n\
         \x20 sproutx add <trg> <repl>   append a rule to the config\n\
         \x20 sproutx test <text>        dry-run expansion of <text>\n\
         \x20 sproutx install            autostart as a systemd user service\n\
         \x20 sproutx uninstall[--keep-config]\n\
         \x20                            remove systemd service (+ config)\n\n\
         config: {}",
        env!("CARGO_PKG_VERSION"),
        config::default_path().display()
    );
    std::process::exit(code);
}

fn run_daemon(verbose: bool) {
    if let Err(e) = daemon::run(verbose) {
        error!("{e}");
        std::process::exit(1);
    }
}

fn cmd_init(force: bool) {
    let path = config::default_path();
    if path.exists() && !force {
        eprintln!("already exists: {}", path.display());
        std::process::exit(1);
    }
    if let Some(p) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(p) {
            eprintln!("cannot create {}: {e}", p.display());
            std::process::exit(1);
        }
    }
    if let Err(e) = std::fs::write(&path, config::sample()) {
        eprintln!("cannot write {}: {e}", path.display());
        std::process::exit(1);
    }
    println!("wrote {}", path.display());
    println!();
    println!("Edit it, then run: sproutx daemon");
}

fn cmd_inspect() {
    println!("input devices (/dev/input):");
    let mut kbd_count = 0usize;
    let rd = match std::fs::read_dir("/dev/input") {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cannot list /dev/input: {e}");
            std::process::exit(1);
        }
    };
    for entry in rd.flatten() {
        let fname = entry.file_name();
        let name = fname.to_string_lossy().to_string();
        if !name.starts_with("event") {
            continue;
        }
        let (dev_name, _caps) = capture::sysfs_info(&name);
        let openable = evdev::Device::open(entry.path()).ok();
        let (is_kbd, openable) = match &openable {
            Some(d) => (capture::is_keyboard(d), true),
            None => (
                dev_name
                    .as_deref()
                    .map(capture::name_looks_like_keyboard)
                    .unwrap_or(false),
                false,
            ),
        };
        let kind = if is_kbd {
            "keyboard"
        } else if dev_name.is_some() {
            "other"
        } else {
            "unknown"
        };
        if is_kbd {
            kbd_count += 1;
        }
        println!(
            "  /dev/input/{name:<7} openable={openable:<5} {kind:<9} {}",
            dev_name.as_deref().unwrap_or("-")
        );
    }
    println!();
    let uinput_ok = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")
        .is_ok();
    println!(
        "injection (/dev/uinput): {}",
        if uinput_ok { "writable" } else { "NOT writable" }
    );
    if !uinput_ok {
        println!("  hint: grant access, e.g. add a udev rule or run:");
        println!("        sudo usermod -aG input $USER");
    }
    if kbd_count == 0 {
        println!();
        println!("no keyboard accessible yet. hint:");
        println!("  sudo usermod -aG input $USER   then log out/in");
    }
}

fn cmd_list() {
    match ipc::send("list") {
        Ok(r) if r.starts_with("ok") => {
            println!("{}", r.trim_end());
        }
        Ok(r) => {
            eprintln!("{r}");
            std::process::exit(1);
        }
        Err(_) => {
            let path = config::default_path();
            match config::load(&path) {
                Ok(cfg) => {
                    println!("ok (from {})", path.display());
                    for rule in &cfg.rules {
                        for t in &rule.triggers {
                            let preview: String = rule.replace.chars().take(60).collect();
                            let suffix = if preview.len() != rule.replace.len() {
                                "..."
                            } else {
                                ""
                            };
                            println!("  {t:?} => {preview}{suffix}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("config: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn cmd_ipc(cmd: &str) {
    match ipc::send(cmd) {
        Ok(r) => println!("{}", r.trim_end()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn cmd_status() {
    match ipc::send("status") {
        Ok(r) => println!("{}", r.trim_end()),
        Err(_) => println!("daemon not running"),
    }
}

fn yaml_quote_trigger(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn cmd_add(trigger: &str, replacement: &str) {
    let path = config::default_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if !path.exists() {
        if let Err(e) = std::fs::write(&path, config::sample()) {
            eprintln!("cannot create {}: {e}", path.display());
            std::process::exit(1);
        }
    }
    let mut out = String::new();
    out.push_str("  - trigger: ");
    out.push_str(&yaml_quote_trigger(trigger));
    out.push_str("\n    replace: |\n");
    for line in replacement.lines() {
        out.push_str("      ");
        out.push_str(line);
        out.push('\n');
    }
    if let Err(e) = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, out.as_bytes()))
    {
        eprintln!("cannot append to {}: {e}", path.display());
        std::process::exit(1);
    }
    println!("added to {}", path.display());
    println!("run `sproutx reload` (if daemon is running)");
}

fn cmd_test(text: &str) {
    let path = config::default_path();
    let cfg = match config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config: {e}");
            std::process::exit(1);
        }
    };
    let eng = engine::Engine::new(&cfg.rules, cfg.depth);
    let mut buffer: Vec<char> = Vec::new();
    for ch in text.chars() {
        if let Some((trig, repl)) = eng.feed_char(&mut buffer, cfg.depth, ch) {
            println!("matched {trig:?} -> {repl:?}");
            println!("       rendered as -> {:?}", engine::render(&repl));
            buffer.clear();
        }
    }
    println!("final buffer: {:?}", buffer.iter().collect::<String>());
}

fn cmd_install() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot locate executable: {e}");
            std::process::exit(1);
        }
    };
    let daemon_path = exe.display();
    let unit = format!(
        "[Unit]\n\
         Description=sproutx system-wide text expander\n\
         After=graphical-session.target\n\
         PartOf=graphical-session.target\n\n\
         [Service]\n\
         ExecStart={daemon_path} daemon\n\
         Restart=on-failure\n\
         RestartSec=2\n\n\
         [Install]\n\
         WantedBy=default.target\n"
    );
    let unit_path = unit_dir().join("sproutx.service");
    if let Err(e) = std::fs::write(&unit_path, unit) {
        eprintln!("cannot write {}: {e}", unit_path.display());
        std::process::exit(1);
    }
    println!("wrote {}", unit_path.display());
    println!();
    println!("now run:");
    println!("  systemctl --user daemon-reload && systemctl --user enable --now sproutx");
    println!(
        "note: reading /dev/input requires the 'input' group: sudo usermod -aG input $USER"
    );
}

fn unit_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from(".config"))
        .join("systemd/user");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn cmd_uninstall(keep_config: bool) {
    let _ = ipc::send("stop");
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", "sproutx"])
        .output();
    let unit_path = unit_dir().join("sproutx.service");
    if unit_path.exists() {
        if let Err(e) = std::fs::remove_file(&unit_path) {
            eprintln!("cannot remove {}: {e}", unit_path.display());
        } else {
            println!("removed {}", unit_path.display());
        }
    }
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    let cfg = config::default_path();
    if let Some(dir) = cfg.parent() {
        if dir.exists() && !cfg.ends_with(".") {
            if keep_config {
                println!("keeping config at {}", dir.display());
            } else {
                match std::fs::remove_dir_all(dir) {
                    Ok(_) => println!("removed {}", dir.display()),
                    Err(e) => eprintln!("cannot remove {}: {e}", dir.display()),
                }
            }
        }
    }
    println!();
    println!("sproutx is uninstalled (kept ~/.cargo/bin/sproutx). clean up with:");
    println!("  cargo uninstall sproutx");
    println!("  sudo usermod -rG input $USER    optionally undo input group");
}