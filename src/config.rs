use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileConfig {
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default = "default_depth")]
    pub depth: usize,
    #[serde(default = "default_delay")]
    pub delay_ms: u64,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

fn de_triggers<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Rule {
    #[serde(default, alias = "trigger", deserialize_with = "de_triggers")]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub replace: String,
}

fn default_depth() -> usize {
    64
}

fn default_delay() -> u64 {
    12
}

pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SPROUTX_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("sproutx").join("config.yaml")
}

pub fn load(path: &Path) -> Result<FileConfig, String> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut cfg: FileConfig =
        serde_yaml::from_str(&s).map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    if cfg.depth == 0 {
        cfg.depth = default_depth();
    }
    if cfg.delay_ms == 0 {
        cfg.delay_ms = default_delay();
    }
    cfg.rules.retain(|r| {
        r.triggers.iter().any(|t| !t.trim().is_empty()) || !r.replace.is_empty()
    });
    Ok(cfg)
}

pub fn resolve_layout(cfg_layout: &Option<String>) -> String {
    if let Some(l) = auto_from_layout(cfg_layout) {
        return l;
    }
    if let Some(l) = auto_from_env() {
        return l;
    }
    if let Some(l) = auto_from_localectl() {
        return l;
    }
    if let Some(l) = auto_from_setxkbmap() {
        return l;
    }
    if let Some(l) = auto_from_def_keyboard() {
        return l;
    }
    "us".to_string()
}

fn auto_from_env() -> Option<String> {
    let v = std::env::var("XKB_DEFAULT_LAYOUT").ok()?;
    let v = v.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

fn auto_from_localectl() -> Option<String> {
    let out = std::process::Command::new("localectl")
        .arg("status")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_localectl(&String::from_utf8_lossy(&out.stdout))
}

fn auto_from_setxkbmap() -> Option<String> {
    let out = std::process::Command::new("setxkbmap")
        .arg("-query")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_setxkbmap(&String::from_utf8_lossy(&out.stdout))
}

fn auto_from_def_keyboard() -> Option<String> {
    let text = std::fs::read_to_string("/etc/default/keyboard").ok()?;
    parse_def_keyboard(&text)
}

fn parse_localectl(text: &str) -> Option<String> {
    let mut layouts: Vec<String> = Vec::new();
    let mut variants: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("X11 Layout:") {
            layouts = split_csv(rest);
        } else if let Some(rest) = line.strip_prefix("X11 Variant:") {
            variants = split_csv(rest);
        }
    }
    join_layouts(&layouts, &variants)
}

fn parse_setxkbmap(text: &str) -> Option<String> {
    let mut layouts: Vec<String> = Vec::new();
    let mut variants: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("layout:") {
            layouts = split_csv(rest);
        } else if let Some(rest) = line.strip_prefix("variant:") {
            variants = split_csv(rest);
        }
    }
    join_layouts(&layouts, &variants)
}

fn parse_def_keyboard(text: &str) -> Option<String> {
    let mut layouts: Vec<String> = Vec::new();
    let mut variants: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("XKBLAYOUT=") {
            layouts = split_csv(rest);
        } else if let Some(rest) = line.strip_prefix("XKBVARIANT=") {
            variants = split_csv(rest);
        }
    }
    join_layouts(&layouts, &variants)
}

/// Split an XKB layout/variant value like `"us, fr"` or `fr(azerty)`.
fn split_csv(s: &str) -> Vec<String> {
    s.trim()
        .trim_matches('"')
        .split(',')
        .map(|x| x.trim().to_string())
        .collect()
}

/// `["us","fr"]` + `["","azerty"]` -> `"us,fr(azerty)"`.
fn join_layouts(layouts: &[String], variants: &[String]) -> Option<String> {
    if layouts.is_empty() || layouts.iter().all(|s| s.is_empty()) {
        return None;
    }
    let parts = layouts
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let v = variants.get(i).map(|s| s.trim()).unwrap_or("");
            if v.is_empty() {
                l.clone()
            } else {
                format!("{l}({v})")
            }
        })
        .collect::<Vec<_>>();
    Some(parts.join(","))
}

fn auto_from_layout(cfg_layout: &Option<String>) -> Option<String> {
    cfg_layout.as_ref().and_then(|l| {
        let l = l.trim();
        if l.is_empty() {
            None
        } else {
            Some(l.to_string())
        }
    })
}

pub fn sample() -> String {
     r#"# sproutx - system-wide text expander
#
#   layout:   XKB layout, e.g. "us", "fr(azerty)", "us,de".
#             null/absent = auto-detect (XKB_DEFAULT_LAYOUT,
#             localectl, setxkbmap, /etc/default/keyboard, else "us").
#   depth:    maximum trigger length to scan for.
#   delay_ms: pause between injected keystrokes (lower = faster, may drop keys).

layout: ~
depth: 64
delay_ms: 12

rules:
  - trigger: ":sproutx"
    replace: "sproutx expands short triggers into long text"

  # a rule can also have several triggers
  - triggers:
      - ":today"
      - ":date"
    replace: "{{today}}"

  - trigger: ":time"
    replace: "{{time}}"

  - trigger: ":now"
    replace: "{{now}}"
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localectl_parses_layout_and_variant() {
        let t = "\
Static hostname: nixos\n\
Other hosts: localhost\n\
  Virtual Console Keymap: (unset)\n\
  Keyboard: fr\n\
       X11 Layout: fr,us\n\
       X11 Variant: azerty,\n\
       X11 Model: pc105\n";
        assert_eq!(parse_localectl(t).unwrap(), "fr(azerty),us");
    }

    #[test]
    fn localectl_ignores_unset_layout() {
        assert_eq!(parse_localectl("  X11 Layout: \n"), None);
    }

    #[test]
    fn setxkbmap_query_parses() {
        let t = "rules:      evdev\nmodel:      pc105\nlayout:     us\nvariant:    intl\n";
        assert_eq!(parse_setxkbmap(t).unwrap(), "us(intl)");
    }

    #[test]
    fn setxkbmap_no_variant_parses() {
        assert_eq!(parse_setxkbmap("layout:     us\n").unwrap(), "us");
    }

    #[test]
    fn def_keyboard_parses_quoted() {
        let t = "XKBLAYOUT=\"us, fr\"\nXKBVARIANT=\"basic, azerty\"\nXKBOPTIONS=terminate:ctrl_alt_bksp\n";
        assert_eq!(parse_def_keyboard(t).unwrap(), "us(basic),fr(azerty)");
    }
}