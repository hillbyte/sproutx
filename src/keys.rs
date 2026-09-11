use std::sync::Arc;
use xkbcommon::xkb::{self, Keycode, Keymap, Keysym, State};

/// Offset between kernel evdev keycodes and XKB keycodes.
pub const XKB_OFFSET: u32 = 8;

pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_ENTER: u16 = 28;
#[allow(dead_code)]
pub const KEY_RIGHTCTRL: u16 = 97;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_RIGHTALT: u16 = 100;
pub const KEY_LEFTMETA: u16 = 125;
pub const KEY_RIGHTMETA: u16 = 126;

/// Modifier keys that signal a shortcut/gesture (context change) rather
/// than plain typing. Shift is deliberately excluded.
pub const HOTKEY_MOD_KEYS: [u16; 6] = [
    KEY_LEFTCTRL,
    KEY_RIGHTCTRL,
    KEY_LEFTALT,
    KEY_RIGHTALT,
    KEY_LEFTMETA,
    KEY_RIGHTMETA,
];

pub fn compile(layout: &str) -> Option<Arc<Keymap>> {
    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    match Keymap::new_from_names(
        &ctx,
        "evdev",
        "pc105",
        layout,
        "",
        None,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    ) {
        Some(k) => Some(Arc::new(k)),
        None => {
            let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
            Keymap::new_from_names(
                &ctx,
                "",
                "",
                layout,
                "",
                None,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
            .map(Arc::new)
        }
    }
}

/// Decodes keystrokes into utf8 text, tracking modifier state.
///
/// Keys pressed while a "hotkey" modifier (Ctrl/Alt/Super) is held are
/// shortcut gestures (copy, paste, workspace switch) rather than text, so
/// they are never fed to the matcher as characters.
pub struct Reader {
    state: State,
    suppress: u32,
}

impl Reader {
    pub fn new(keymap: &Arc<Keymap>) -> Reader {
        let idx = |n: &str| -> u32 {
            let i = keymap.mod_get_index(n);
            if i != u32::MAX && i < 32 {
                1u32 << i
            } else {
                0
            }
        };
        let mut suppress = 0u32;
        for n in ["Control", "Mod1", "Alt", "Mod3", "Mod4", "Meta", "Super"] {
            suppress |= idx(n);
        }
        Reader {
            state: State::new(keymap),
            suppress,
        }
    }

    /// True when a Ctrl/Alt/Super-type modifier is currently held down
    /// (Shift, CapsLock and AltGr/Mod2 are typing modifiers, so they are
    /// deliberately excluded).
    fn hotkey_active(&self) -> bool {
        const ACTIVE: u32 = xkb::STATE_MODS_DEPRESSED | xkb::STATE_MODS_LATCHED;
        self.suppress != 0 && (self.suppress & self.state.serialize_mods(ACTIVE)) != 0
    }

    /// value: 0 = release, 1 = press, 2 = repeat.
    /// Returns one printable utf8 string when a key press produces text.
    pub fn on_event(&mut self, evcode: u16, value: i32) -> Option<String> {
        let kc = Keycode::new(evcode as u32 + XKB_OFFSET);
        match value {
            0 => {
                self.state.update_key(kc, xkb::KeyDirection::Up);
                None
            }
            1 => {
                self.state.update_key(kc, xkb::KeyDirection::Down);
                if self.hotkey_active() {
                    return None;
                }
                let s = self.state.key_get_utf8(kc);
                if s.is_empty() || s.chars().all(|c| c.is_control()) {
                    None
                } else {
                    Some(s)
                }
            }
            _ => {
                self.state.update_key(kc, xkb::KeyDirection::Down);
                None
            }
        }
    }
}

/// A single keystroke plus modifiers to hold while typing it.
#[derive(Debug, Clone)]
pub struct Stroke {
    pub key: u16,
    pub mods: Vec<u16>,
}

struct ModBits {
    shift: u32,
    lock: u32,
    ctrl: u32,
    mod1: u32,
    mod2: u32,
    meta: u32,
}

/// Converts unicode chars into concrete keystrokes for the current keymap.
pub struct Planner {
    pub keymap: Arc<Keymap>,
    mods: ModBits,
}

impl Planner {
    pub fn new(keymap: &Arc<Keymap>) -> Planner {
        let idx = |names: &[&str]| -> u32 {
            for n in names {
                let i = keymap.mod_get_index(n);
                if i != u32::MAX && i < 32 {
                    return 1u32 << i;
                }
            }
            0
        };
        Planner {
            mods: ModBits {
                shift: idx(&["Shift"]),
                lock: idx(&["Lock", "CapsLock"]),
                ctrl: idx(&["Control"]),
                mod1: idx(&["Mod1", "Alt"]),
                mod2: idx(&["Mod2", "ISO_Level3_Shift", "AltGr"]),
                meta: idx(&["Mod4", "Super", "Meta"]),
            },
            keymap: keymap.clone(),
        }
    }

    /// Returns zero or more strokes needed to type `ch` using the current keymap.
    pub fn stroke_for(&self, ch: char) -> Option<Vec<Stroke>> {
        match ch {
            '\n' => return Some(vec![Stroke { key: KEY_ENTER, mods: vec![] }]),
            '\t' => return Some(vec![Stroke { key: KEY_TAB, mods: vec![] }]),
            _ => {}
        }
        let target = Keysym::from_char(ch).raw();
        if target == 0 {
            return None;
        }

        let mut best: Option<(Stroke, u32)> = None;
        let lo = self.keymap.min_keycode().raw();
        let hi = self.keymap.max_keycode().raw();
        for k in lo..=hi {
            let nlay = self.keymap.num_layouts_for_key(Keycode::new(k));
            for lay in 0..nlay {
                let nlev = self.keymap.num_levels_for_key(Keycode::new(k), lay);
                for lev in 0..nlev {
                    let syms = self.keymap.key_get_syms_by_level(Keycode::new(k), lay, lev);
                    if !syms.iter().any(|s| s.raw() == target) {
                        continue;
                    }
                    let mut masks = [0u32; 96];
                    let nm =
                        self.keymap.key_get_mods_for_level(Keycode::new(k), lay, lev, &mut masks);
                    for mask in masks[..nm.min(96)].iter() {
                        if let Some(mods) = self.translate(*mask) {
                            let score = mods.len() as u32 + lay * 16 + lev * 8;
                            if best
                                .as_ref()
                                .map(|(_, s)| score < *s)
                                .unwrap_or(true)
                            {
                                best = Some((
                                    Stroke {
                                        key: (k - XKB_OFFSET) as u16,
                                        mods,
                                    },
                                    score,
                                ));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(s, _)| vec![s])
    }

    fn translate(&self, mask: u32) -> Option<Vec<u16>> {
        if mask == 0 {
            return Some(vec![]);
        }
        let mut kcs = Vec::new();
        for bit in 0..32u8 {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let b = 1u32 << bit;
            if b == self.mods.shift {
                kcs.push(KEY_LEFTSHIFT);
            } else if self.mods.ctrl != 0 && b == self.mods.ctrl {
                return None;
            } else if self.mods.lock != 0 && b == self.mods.lock {
                return None;
            } else if self.mods.mod1 != 0 && b == self.mods.mod1 {
                kcs.push(KEY_LEFTALT);
            } else if self.mods.mod2 != 0 && b == self.mods.mod2 {
                kcs.push(KEY_RIGHTALT);
            } else if self.mods.meta != 0 && b == self.mods.meta {
                kcs.push(KEY_LEFTMETA);
            } else {
                return None;
            }
        }
        if kcs.is_empty() {
            None
        } else {
            Some(kcs)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn reader() -> Reader {
        Reader::new(&compile("us").unwrap())
    }

    #[test]
    fn decodes_plain_letter() {
        assert_eq!(reader().on_event(30, 1).unwrap(), "a");
    }

    #[test]
    fn decodes_shifted_letter() {
        let mut r = reader();
        r.on_event(KEY_LEFTSHIFT, 1);
        assert_eq!(r.on_event(30, 1).unwrap(), "A");
    }

    #[test]
    fn suppresses_ctrl_chord() {
        let mut r = reader();
        r.on_event(KEY_LEFTCTRL, 1);
        assert!(r.on_event(30, 1).is_none());
    }

    #[test]
    fn suppresses_super_chord() {
        let mut r = reader();
        r.on_event(KEY_LEFTMETA, 1);
        assert!(r.on_event(2, 1).is_none());
    }

    #[test]
    fn suppresses_alt_chord() {
        let mut r = reader();
        r.on_event(KEY_LEFTALT, 1);
        assert!(r.on_event(30, 1).is_none());
    }

    #[test]
    fn shift_still_allows_chars_after_ctrl_release() {
        let mut r = reader();
        r.on_event(KEY_LEFTCTRL, 1);
        r.on_event(KEY_LEFTCTRL, 0);
        assert_eq!(r.on_event(30, 1).unwrap(), "a");
    }
}
