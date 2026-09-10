use evdev::{enumerate, Device, KeyCode};
use std::path::PathBuf;

const VKB_NAME_MARK: &str = "sproutx";

fn looks_like_our_device(name: &str) -> bool {
    name.to_ascii_lowercase().contains(VKB_NAME_MARK)
}

pub fn is_keyboard(dev: &Device) -> bool {
    if dev.name().map(looks_like_our_device).unwrap_or(false) {
        return false;
    }
    let Some(keys) = dev.supported_keys() else {
        return false;
    };
    [
        KeyCode::KEY_A,
        KeyCode::KEY_Z,
        KeyCode::KEY_ENTER,
        KeyCode::KEY_LEFTSHIFT,
        KeyCode::KEY_SPACE,
    ]
    .iter()
    .all(|k| keys.contains(*k))
}

pub fn open_keyboards() -> Vec<(PathBuf, Device)> {
    enumerate().filter(|(_, d)| is_keyboard(d)).collect()
}

pub fn open_permission_denied() -> bool {
    if let Ok(rd) = std::fs::read_dir("/dev/input") {
        for entry in rd.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("event") {
                if let Err(e) = Device::open(entry.path()) {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn sysfs_info(event_name: &str) -> (Option<String>, Option<String>) {
    let sysdir = format!("/sys/class/input/{event_name}");
    let name = std::fs::read_to_string(format!("{sysdir}/device/name"))
        .ok()
        .map(|s| s.trim().to_string());
    let caps = std::fs::read_to_string(format!("{sysdir}/device/capabilities/key")).ok();
    (name, caps)
}

pub fn name_looks_like_keyboard(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("keyboard") || n.contains("kbd")
}