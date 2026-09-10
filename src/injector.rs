use crate::keys::{Planner, Stroke, KEY_BACKSPACE};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};
use std::io;
use std::thread;
use std::time::Duration;

pub const VDEV_NAME: &str = "sproutx-vkbd";

pub struct Injector {
    dev: VirtualDevice,
}

impl Injector {
    pub fn create() -> io::Result<Injector> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for k in 1u16..=0x02ff {
            keys.insert(KeyCode::new(k));
        }
        let dev = VirtualDevice::builder()?
            .name(VDEV_NAME)
            .with_keys(&keys)?
            .build()?;
        Ok(Injector { dev })
    }

    fn write(&mut self, code: u16, value: i32) -> io::Result<()> {
        self.dev.emit(&[InputEvent::new(EventType::KEY.0, code, value)])
    }

    fn tap(&mut self, code: u16, delay: Duration) -> io::Result<()> {
        self.write(code, 1)?;
        thread::sleep(delay);
        self.write(code, 0)?;
        thread::sleep(delay);
        Ok(())
    }

    fn stroke(&mut self, s: &Stroke, delay: Duration) -> io::Result<()> {
        for &m in &s.mods {
            self.write(m, 1)?;
            thread::sleep(delay);
        }
        self.write(s.key, 1)?;
        thread::sleep(delay);
        self.write(s.key, 0)?;
        thread::sleep(delay);
        for &m in s.mods.iter().rev() {
            self.write(m, 0)?;
            thread::sleep(delay);
        }
        Ok(())
    }

    pub fn backspace_n(&mut self, n: usize, delay: Duration) {
        for _ in 0..n.max(0) {
            if let Err(e) = self.tap(KEY_BACKSPACE, delay) {
                error!("inject backspace: {e}");
                return;
            }
        }
    }

    pub fn type_text(&mut self, planner: &Planner, text: &str, delay: Duration) {
        for ch in text.chars() {
            match planner.stroke_for(ch) {
                Some(strokes) => {
                    for s in &strokes {
                        if let Err(e) = self.stroke(s, delay) {
                            error!("inject {ch:?}: {e}");
                            return;
                        }
                    }
                }
                None => warn!("cannot type '{ch}' with the current layout; skipped"),
            }
        }
    }
}