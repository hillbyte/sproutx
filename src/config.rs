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
    if let Some(l) = auto_from_setxkbmap() {
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

fn auto_from_setxkbmap() -> Option<String> {
    let out = std::process::Command::new("setxkbmap")
        .arg("-query")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut layouts: Vec<String> = Vec::new();
    let mut variants: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("layout:") {
            layouts = rest.split(',').map(|s| s.trim().to_string()).collect();
        } else if let Some(rest) = line.strip_prefix("variant:") {
            variants = rest.split(',').map(|s| s.trim().to_string()).collect();
        }
    }
    if layouts.is_empty() || layouts.iter().all(|s| s.is_empty()) {
        return None;
    }
    let joined = layouts
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let v = variants.get(i).cloned().unwrap_or_default();
            if v.is_empty() {
                l.clone()
            } else {
                format!("{l}({v})")
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    Some(joined)
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
#             null/absent = auto-detect (setxkbmap / XKB_DEFAULT_LAYOUT, else "us").
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