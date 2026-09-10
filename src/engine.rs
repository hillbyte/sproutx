use crate::config::Rule;
use chrono::{Days, Local, NaiveDateTime, Timelike};

pub struct Engine {
    sorted: Vec<(String, String)>,
    ambiguous: std::collections::HashSet<String>,
    depth: usize,
}

impl Engine {
    pub fn new(rules: &[Rule], depth: usize) -> Engine {
        let mut sorted: Vec<(String, String)> = rules
            .iter()
            .flat_map(|r| {
                r.triggers
                    .iter()
                    .map(move |t| (t.trim().to_string(), r.replace.clone()))
            })
            .filter(|(t, _)| !t.is_empty())
            .collect();
        sorted.sort_by(|a, b| {
            b.0.chars()
                .count()
                .cmp(&a.0.chars().count())
                .then_with(|| a.0.cmp(&b.0))
        });
        let triggers: Vec<&str> = sorted.iter().map(|(t, _)| t.as_str()).collect();
        let mut ambiguous = std::collections::HashSet::new();
        for (i, a) in triggers.iter().enumerate() {
            for b in triggers.iter().skip(i + 1) {
                // Only the *shorter* trigger of a prefix pair needs to wait for
                // a delimiter, so that `:t` does not hijack `:time`.
                let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
                if long.starts_with(short) {
                    ambiguous.insert(short.to_string());
                }
            }
        }
        Engine {
            sorted,
            ambiguous,
            depth: if depth == 0 { 64 } else { depth },
        }
    }

    pub fn rules(&self) -> &[(String, String)] {
        &self.sorted
    }

    /// A trigger is "ambiguous" when it is a strict prefix of (or shares
    /// prefix overlap with) a longer trigger. Ambiguous triggers only fire
    /// when followed by a delimiter, so that `:t` does not hijack `:time`.
    pub fn is_ambiguous(&self, trig: &str) -> bool {
        self.ambiguous.contains(trig)
    }

    /// Exact match: the trigger must occupy buffer[start..n) with a word
    /// boundary before it.
    fn match_window(&self, buffer: &[char], n: usize) -> Option<(String, String)> {
        if n > buffer.len() {
            return None;
        }
        for (trig, repl) in &self.sorted {
            let tl = trig.chars().count();
            if tl > n || tl > self.depth {
                continue;
            }
            let start = n - tl;
            if !buffer[start..n]
                .iter()
                .zip(trig.chars())
                .all(|(a, b)| *a == b)
            {
                continue;
            }
            if start > 0 {
                let prev = buffer[start - 1];
                if prev.is_alphanumeric() || prev == '_' {
                    continue;
                }
            }
            return Some((trig.clone(), repl.clone()));
        }
        None
    }

    /// Same as match_window but only for unambiguous triggers (immediate firing).
    fn match_window_immediate(&self, buffer: &[char], n: usize) -> Option<(String, String)> {
        match self.match_window(buffer, n) {
            Some((t, r)) if !self.is_ambiguous(&t) => Some((t, r)),
            _ => None,
        }
    }

    /// Push one typed char into `buffer` (respecting depth) and return any
    /// expansion that should happen *now*, if the matcher spawned one.
    pub fn feed_char(
        &self,
        buffer: &mut Vec<char>,
        depth: usize,
        ch: char,
    ) -> Option<(String, String)> {
        let is_delim = !ch.is_alphanumeric() && ch != '_';
        buffer.push(ch);
        let cap = depth.max(1) + 1;
        if buffer.len() > cap {
            let excess = buffer.len() - cap;
            buffer.drain(0..excess);
            if is_delim {
                if let Some(m) = self.match_window(buffer, buffer.len()) {
                    return Some(m);
                }
            }
            return None;
        }
        if is_delim {
            // Delimiter typed: try a trigger that itself ends with the delimiter
            // (e.g. ":date "), otherwise one ending right before it (":date " -> "date").
            if let Some(m) = self.match_window(buffer, buffer.len()) {
                return Some(m);
            }
            self.match_window(buffer, buffer.len().saturating_sub(1))
        } else {
            self.match_window_immediate(buffer, buffer.len())
        }
    }

    /// Called on Enter / Escape: look for a completed trigger at the very end.
    pub fn match_at_end(&self, buffer: &[char]) -> Option<(String, String)> {
        self.match_window(buffer, buffer.len())
    }
}

pub fn render(input: &str) -> String {
    let now = Local::now().naive_local();
    let mut out = String::new();
    let mut rest = input;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 2..];
        if let Some(close) = tail.find("}}") {
            let var = &tail[..close];
            out.push_str(&render_var(var.trim(), now));
            rest = &tail[close + 2..];
        } else {
            out.push_str("{{");
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

fn render_var(var: &str, now: NaiveDateTime) -> String {
    match var {
        "today" | "date" => fmt_tokens(&now, "%Y-%m-%d"),
        "time" => fmt_tokens(&now, "%H:%M:%S"),
        "now" => fmt_tokens(&now, "%Y-%m-%d %H:%M:%S"),
        v if v.starts_with("today") || v.starts_with("date") => {
            let base = if v.starts_with("today") {
                v["today".len()..].trim()
            } else {
                v["date".len()..].trim()
            };
            let (offset, fmt) = match base.split_once(':') {
                Some((o, f)) => (o.trim(), f.trim()),
                None => (base, ""),
            };
            let days: i64 = if offset.is_empty() {
                0
            } else if let Ok(n) = offset.parse::<i64>() {
                n
            } else {
                return format!("{{{{{var}}}}}");
            };
            let dt = if days >= 0 {
                now.checked_add_days(Days::new(days as u64))
            } else {
                now.checked_sub_days(Days::new((-days) as u64))
            };
            let dt = dt.unwrap_or(now);
            fmt_tokens(&dt, if fmt.is_empty() { "%Y-%m-%d" } else { fmt })
        }
        v if v.starts_with("time:") => {
            let fmt = v["time:".len()..].trim();
            let fmt = if fmt.is_empty() { "%H:%M:%S" } else { fmt };
            fmt_tokens(&now, fmt)
        }
        v if v.starts_with("date:") => {
            let fmt = v["date:".len()..].trim();
            let fmt = if fmt.is_empty() { "%Y-%m-%d" } else { fmt };
            fmt_tokens(&now, fmt)
        }
        other => format!("{{{{{other}}}}}"),
    }
}

fn fmt_tokens(dt: &NaiveDateTime, fmt: &str) -> String {
    use chrono::Datelike;
    const MONTHS: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September",
        "October", "November", "December",
    ];
    const DAYS: [&str; 7] = [
        "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
    ];
    let ordinal = dt.ordinal0();
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&format!("{:04}", dt.year())),
            Some('y') => out.push_str(&format!("{:02}", dt.year() % 100)),
            Some('m') => out.push_str(&format!("{:02}", dt.month())),
            Some('d') => out.push_str(&format!("{:02}", dt.day())),
            Some('e') => out.push_str(&format!("{:2}", dt.day())),
            Some('H') => out.push_str(&format!("{:02}", dt.hour())),
            Some('M') => out.push_str(&format!("{:02}", dt.minute())),
            Some('S') => out.push_str(&format!("{:02}", dt.second())),
            Some('j') => out.push_str(&format!("{:03}", ordinal + 1)),
            Some('b') => out.push_str(&MONTHS[dt.month0() as usize][..3]),
            Some('B') => out.push_str(MONTHS[dt.month0() as usize]),
            Some('a') => out.push_str(&DAYS[dt.weekday().num_days_from_monday() as usize][..3]),
            Some('A') => out.push_str(DAYS[dt.weekday().num_days_from_monday() as usize]),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}