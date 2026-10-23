//! Developer diagnostics across a Connect room.
//!
//! A device with developer mode on can ask the rest of its room for their
//! diagnostics bundles (`Query::RoomDiagnostics`). The request goes to the
//! room, which answers at once with its own state ([`ROOM`]) and the list of
//! devices it passed the request on to. Each of those answers with its
//! platform's bundle while its own developer mode is on, and declines
//! otherwise. A hosted coordinator also answers as [`COORDINATOR`]: its version
//! for everyone, and its whole process log for the accounts its operator named
//! with `--diagnostics-user`.
//!
//! Answers can run to megabytes, so they are capped ([`cap`]) and sent in
//! parts ([`split`]). A device sends one part per engine tick, so a big answer
//! never holds up its heartbeats. [`Collection`] puts the answers together on
//! the device that asked.

use crate::api::{DeviceId, DeviceInfo, EpochMs};

/// `from` of the room's own answer.
pub const ROOM: &str = "room";
/// `from` of a hosted coordinator's answer.
pub const COORDINATOR: &str = "coordinator";
/// Largest answer one source sends ([`cap`] keeps its head and its tail).
pub const MAX_ANSWER_BYTES: usize = 4 << 20;
/// Largest part, measured as JSON-escaped text: well under the 1 MiB frame cap
/// with the envelope around it.
pub const PART_BYTES: usize = 256 << 10;
/// What [`cap`] keeps from the start of an answer (the bundle's header).
const HEAD_BYTES: usize = 64 << 10;
/// Parts accepted per source; anything past it is dropped.
pub const MAX_PARTS: u32 = 64;
/// Sources accepted per collection.
const MAX_SOURCES: usize = 64;
/// How long the asking device waits for answers.
pub const COLLECT_TIMEOUT_MS: f64 = 45_000.0;
/// How long a device waits for its platform to build its bundle before it
/// answers with the core's own diagnostics and log instead.
pub const PLATFORM_ANSWER_TIMEOUT_MS: f64 = 10_000.0;
/// A room passes on at most one request per member this often.
pub const REQUEST_INTERVAL_MS: f64 = 10_000.0;

/// One part of an answer, as the asking device receives it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyPart {
    pub request_id: String,
    pub from: String,
    pub from_name: String,
    pub part: u32,
    pub parts: u32,
    pub text: String,
    pub declined: Option<String>,
    pub asked: Vec<DeviceInfo>,
}

/// `text` cut to at most `max` bytes: its start (up to [`HEAD_BYTES`]) and its end, with a line
/// saying how much was left out. Both cuts land on line breaks where there is one nearby.
pub fn cap(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let head_end = floor_boundary(text, HEAD_BYTES.min(max / 4));
    let head_end = text[..head_end].rfind('\n').map_or(head_end, |i| i + 1);
    // Room for the marker line.
    let tail_len = max.saturating_sub(head_end + 64);
    let tail_start = ceil_boundary(text, text.len() - tail_len.min(text.len() - head_end));
    let tail_start = text[tail_start..]
        .find('\n')
        .map_or(tail_start, |i| tail_start + i + 1);
    let left_out = tail_start - head_end;
    format!(
        "{}[… {left_out} bytes left out …]\n{}",
        &text[..head_end],
        &text[tail_start..]
    )
}

/// `text` in parts whose JSON-escaped size is at most `part_bytes`, cut on character boundaries.
/// Always at least one part (empty text is one empty part).
pub fn split(text: &str, part_bytes: usize) -> Vec<String> {
    let mut parts = vec![];
    let mut start = 0;
    let mut size = 0;
    for (i, c) in text.char_indices() {
        let cost = escaped_len(c);
        if size + cost > part_bytes && i > start {
            parts.push(text[start..i].to_string());
            start = i;
            size = 0;
        }
        size += cost;
    }
    if start < text.len() || parts.is_empty() {
        parts.push(text[start..].to_string());
    }
    parts
}

/// Bytes `c` takes inside a JSON string.
fn escaped_len(c: char) -> usize {
    match c {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[derive(Debug, Clone)]
struct Source {
    id: String,
    name: String,
    parts: Vec<Option<String>>,
    declined: Option<String>,
}

impl Source {
    fn complete(&self) -> bool {
        self.declined.is_some() || self.parts.iter().all(Option::is_some)
    }

    fn received(&self) -> usize {
        self.parts.iter().filter(|p| p.is_some()).count()
    }

    fn text(&self) -> String {
        self.parts.iter().flatten().map(String::as_str).collect()
    }
}

/// The answers to one request, on the device that asked. Complete once the room has answered,
/// every device it asked has answered or declined, and every other source that started (the
/// coordinator) has finished.
#[derive(Debug, Clone)]
pub struct Collection {
    pub request_id: String,
    started_at: EpochMs,
    room_answered: bool,
    asked: Vec<(DeviceId, String)>,
    sources: Vec<Source>,
}

impl Collection {
    pub fn new(request_id: impl Into<String>, now: EpochMs) -> Self {
        Collection {
            request_id: request_id.into(),
            started_at: now,
            room_answered: false,
            asked: vec![],
            sources: vec![],
        }
    }

    pub fn add(&mut self, p: ReplyPart) {
        if p.request_id != self.request_id || p.parts == 0 || p.part >= p.parts {
            return;
        }
        if p.from == ROOM && p.part == 0 {
            self.room_answered = true;
            for d in &p.asked {
                if !self.asked.iter().any(|(id, _)| *id == d.id) {
                    self.asked.push((d.id.clone(), d.name.clone()));
                }
            }
        }
        let i = match self.sources.iter().position(|s| s.id == p.from) {
            Some(i) => i,
            None if self.sources.len() >= MAX_SOURCES => return,
            None => {
                let parts = p.parts.min(MAX_PARTS) as usize;
                self.sources.push(Source {
                    id: p.from.clone(),
                    name: p.from_name.clone(),
                    parts: vec![None; parts],
                    declined: None,
                });
                self.sources.len() - 1
            }
        };
        let s = &mut self.sources[i];
        if let Some(reason) = p.declined {
            s.declined = Some(reason);
            return;
        }
        if let Some(slot) = s.parts.get_mut(p.part as usize) {
            *slot = Some(p.text);
        }
    }

    pub fn is_complete(&self) -> bool {
        self.room_answered
            && self
                .asked
                .iter()
                .all(|(id, _)| self.sources.iter().any(|s| s.id == *id && s.complete()))
            && self.sources.iter().all(Source::complete)
    }

    pub fn expired(&self, now: EpochMs) -> bool {
        now - self.started_at >= COLLECT_TIMEOUT_MS
    }

    /// The answers as one text, for the end of the asking device's bundle.
    pub fn render(&self, now: EpochMs) -> String {
        let devices: Vec<&(DeviceId, String)> = self.asked.iter().collect();
        let answered = devices
            .iter()
            .filter(|(id, _)| {
                self.sources
                    .iter()
                    .any(|s| s.id == *id && s.complete() && s.declined.is_none())
            })
            .count();
        let mut out = String::new();
        let title = format!(
            "Other devices in the Connect room ({answered} of {} sent a bundle, {:.1} s)",
            devices.len(),
            ((now - self.started_at) / 1000.0).max(0.0)
        );
        out.push_str(&title);
        out.push('\n');
        out.push_str(&"=".repeat(title.chars().count()));
        out.push_str("\n\n");
        if !self.room_answered {
            out.push_str("The room did not answer: is this device connected to anything?\n\n");
        }
        let mut order: Vec<&Source> = vec![];
        for id in [ROOM, COORDINATOR] {
            order.extend(self.sources.iter().filter(|s| s.id == id));
        }
        for (id, _) in &self.asked {
            order.extend(self.sources.iter().filter(|s| s.id == *id));
        }
        let rest: Vec<&Source> = self
            .sources
            .iter()
            .filter(|s| !order.iter().any(|o| o.id == s.id))
            .collect();
        order.extend(rest);
        for s in order {
            let heading = match s.id.as_str() {
                ROOM => "Room".to_string(),
                COORDINATOR => "Coordinator".to_string(),
                id => format!("Device: {} ({id})", s.name),
            };
            section(&mut out, &heading);
            if let Some(reason) = &s.declined {
                out.push_str(&format!("(declined: {reason})\n\n"));
                continue;
            }
            if !s.complete() {
                out.push_str(&format!(
                    "(incomplete: {} of {} parts arrived)\n",
                    s.received(),
                    s.parts.len()
                ));
            }
            out.push_str(s.text().trim_end());
            out.push_str("\n\n");
        }
        for (id, name) in &self.asked {
            if !self.sources.iter().any(|s| s.id == *id) {
                section(&mut out, &format!("Device: {name} ({id})"));
                out.push_str("(no answer)\n\n");
            }
        }
        out
    }
}

fn section(out: &mut String, heading: &str) {
    out.push_str(heading);
    out.push('\n');
    out.push_str(&"-".repeat(heading.chars().count()));
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Platform;

    fn device(id: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            name: format!("Device {id}"),
            platform: Platform::Linux,
            app_version: "0.1".into(),
            playing: false,
            ready: false,
            last_seen: 0.0,
            is_self: false,
        }
    }

    fn part(from: &str, part: u32, parts: u32, text: &str) -> ReplyPart {
        ReplyPart {
            request_id: "r".into(),
            from: from.into(),
            from_name: format!("Name {from}"),
            part,
            parts,
            text: text.into(),
            declined: None,
            asked: vec![],
        }
    }

    #[test]
    fn split_bounds_escaped_size_and_round_trips() {
        let text = "a\"b\n\u{1}é€😀".repeat(5_000);
        let parts = split(&text, 1000);
        assert!(parts.len() > 1);
        for p in &parts {
            let escaped = serde_json::to_string(p).unwrap();
            assert!(escaped.len() <= 1000 + 2, "{}", escaped.len());
        }
        assert_eq!(parts.concat(), text);
        assert_eq!(split("", 10), vec![String::new()]);
    }

    #[test]
    fn cap_keeps_head_and_tail() {
        let text: String = (0..200_000).map(|i| format!("line {i}\n")).collect();
        let capped = cap(&text, 100_000);
        assert!(capped.len() <= 100_000, "{}", capped.len());
        assert!(capped.starts_with("line 0\n"));
        assert!(capped.ends_with("line 199999\n"));
        assert!(capped.contains("bytes left out"));
        assert_eq!(cap("short", 100), "short");
    }

    #[test]
    fn collection_waits_for_the_room_the_asked_and_started_sources() {
        let mut c = Collection::new("r", 0.0);
        assert!(!c.is_complete());
        c.add(part(COORDINATOR, 0, 2, "coord-a "));
        let mut room = part(ROOM, 0, 1, "room state");
        room.asked = vec![device("b"), device("c")];
        c.add(room);
        assert!(!c.is_complete());
        c.add(part("b", 1, 2, "world"));
        c.add(part("b", 0, 2, "hello "));
        let mut declined = part("c", 0, 1, "");
        declined.declined = Some("developer mode is off on this device".into());
        c.add(declined);
        assert!(!c.is_complete(), "the coordinator has one part to go");
        c.add(part(COORDINATOR, 1, 2, "coord-b"));
        assert!(c.is_complete());
        let text = c.render(2_000.0);
        assert!(text.contains("1 of 2 sent a bundle"), "{text}");
        assert!(text.contains("hello world"));
        assert!(text.contains("coord-a coord-b"));
        assert!(text.contains("(declined: developer mode is off"));
        let room_at = text.find("Room\n").unwrap();
        let coord_at = text.find("Coordinator\n").unwrap();
        let b_at = text.find("Device: Name b (b)").unwrap();
        assert!(room_at < coord_at && coord_at < b_at, "{text}");
    }

    #[test]
    fn collection_reports_silence_and_ignores_strays() {
        let mut c = Collection::new("r", 0.0);
        let mut room = part(ROOM, 0, 1, "state");
        room.asked = vec![device("b")];
        c.add(room);
        c.add(ReplyPart {
            request_id: "other".into(),
            ..part("b", 0, 1, "stray")
        });
        c.add(part("b", 5, 2, "out of range"));
        assert!(!c.is_complete());
        assert!(c.expired(COLLECT_TIMEOUT_MS));
        let text = c.render(COLLECT_TIMEOUT_MS);
        assert!(text.contains("Device: Device b (b)\n"), "{text}");
        assert!(text.contains("(no answer)"));
        assert!(!text.contains("stray"));
    }
}
