use std::sync::atomic::{AtomicU8, Ordering};

static LEVEL: AtomicU8 = AtomicU8::new(2);

pub fn init(verbose: bool) {
    let level = match std::env::var("SPROUTX_LOG") {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "off" | "error" => 0,
            "warn" | "warning" => 1,
            "info" => 2,
            "debug" | "trace" => 3,
            _ => 2,
        },
        Err(_) if verbose => 3,
        Err(_) => 2,
    };
    LEVEL.store(level, Ordering::Relaxed);
}

pub fn enabled(level: u8) -> bool {
    LEVEL.load(Ordering::Relaxed) >= level
}

pub fn emit(level: u8, args: std::fmt::Arguments) {
    if !enabled(level) {
        return;
    }
    let tag = match level {
        0 => "error",
        1 => "warn",
        2 => "info",
        _ => "debug",
    };
    eprintln!("[sproutx:{tag}] {args}");
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::emit(0, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::emit(1, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::emit(2, format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::log::emit(3, format_args!($($arg)*))
    };
}