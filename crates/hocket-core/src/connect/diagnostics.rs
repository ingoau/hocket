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
//! Answers can run to megabytes, so they are capped and sent in parts
//! ([`parts`]). A device sends one part per engine tick (about 1 Mbit/s), so a
//! big answer never holds up its heartbeats. [`Collection`] puts the answers
//! together on the device that asked.
//!
//! Devices that know the request say so in their `Hello` ([`HELLO_FLAG`] in
//! its extra fields): the room asks only those, and names the others in its
//! own answer, so an older build never leaves the asking device waiting.

use crate::api::{
    DeviceId, DeviceInfo, DiagnosticsSourceState, DiagnosticsSourceStatus, EpochMs,
    RoomDiagnosticsStatus,
};

/// `from` of the room's own answer.
pub const ROOM: &str = "room";
/// `from` of a hosted coordinator's answer.
pub const COORDINATOR: &str = "coordinator";
/// What starts the process log in a coordinator's answer: it sends one only
/// to the accounts its operator named.
pub const COORDINATOR_LOG_HEADING: &str = "Process log (last ";
/// `Hello` extra field (`true`) of a device that answers diagnostics requests.
pub const HELLO_FLAG: &str = "diagnostics";
/// Largest answer one source sends ([`cap`] keeps its head and its tail).
pub const MAX_ANSWER_BYTES: usize = 2 << 20;
/// Largest part, measured as JSON-escaped text: well under the 1 MiB frame cap
/// with the envelope around it, and one a second is about 1 Mbit/s.
pub const PART_BYTES: usize = 128 << 10;
/// What [`cap`] keeps from the start of an answer (the bundle's header).
const HEAD_BYTES: usize = 64 << 10;
/// Parts accepted per source; anything past it is dropped.
pub const MAX_PARTS: u32 = 64;
/// Sources accepted per collection.
const MAX_SOURCES: usize = 64;
/// Text a collection keeps in all; parts past it are dropped.
const MAX_COLLECTION_BYTES: usize = 64 << 20;
/// How far [`cap`] looks for a line break to cut at.
const SNAP_BYTES: usize = 1024;
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

/// Whether a device id would pass for the room's or the coordinator's answer.
pub fn is_reserved(id: &str) -> bool {
    id == ROOM || id == COORDINATOR
}

/// An answer as it goes on the wire: capped to [`MAX_ANSWER_BYTES`] and split into at most
/// [`MAX_PARTS`] parts of [`PART_BYTES`] (text that escapes badly is capped harder).
pub fn parts(text: &str) -> Vec<String> {
    let mut max = MAX_ANSWER_BYTES;
    loop {
        let parts = split(&cap(text, max), PART_BYTES);
        if parts.len() <= MAX_PARTS as usize || max <= PART_BYTES {
            return parts.into_iter().take(MAX_PARTS as usize).collect();
        }
        max /= 2;
    }
}

/// `text` cut to at most `max` bytes: its start (up to [`HEAD_BYTES`]) and its end, with a line
/// saying how much was left out. Both cuts land on a line break within [`SNAP_BYTES`] if there is
/// one.
pub fn cap(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let head_end = floor_boundary(text, HEAD_BYTES.min(max / 4));
    let head_end = text[..head_end]
        .rfind('\n')
        .filter(|i| head_end - i <= SNAP_BYTES)
        .map_or(head_end, |i| i + 1);
    // Room for the marker line.
    let tail_len = max.saturating_sub(head_end + 64);
    let tail_start = ceil_boundary(text, text.len() - tail_len.min(text.len() - head_end));
    let tail_start = text[tail_start..]
        .find('\n')
        .filter(|&i| i < SNAP_BYTES && tail_start + i + 1 < text.len())
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

    fn status(&self, done: bool) -> DiagnosticsSourceStatus {
        let received = self.received();
        let state = if self.declined.is_some() {
            DiagnosticsSourceState::Declined
        } else if self.complete() {
            DiagnosticsSourceState::Received
        } else if done {
            DiagnosticsSourceState::Incomplete
        } else {
            DiagnosticsSourceState::Receiving
        };
        DiagnosticsSourceStatus {
            id: self.id.clone(),
            name: self.name.clone(),
            state,
            parts_received: received as u32,
            parts: self.parts.len() as u32,
            bytes: self.parts.iter().flatten().map(String::len).sum::<usize>() as u32,
            reason: self.declined.clone(),
        }
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
    /// Text held so far.
    bytes: usize,
}

impl Collection {
    pub fn new(request_id: impl Into<String>, now: EpochMs) -> Self {
        Collection {
            request_id: request_id.into(),
            started_at: now,
            room_answered: false,
            asked: vec![],
            sources: vec![],
            bytes: 0,
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
                let too_many = p.parts > MAX_PARTS;
                self.sources.push(Source {
                    id: p.from.clone(),
                    name: p.from_name.clone(),
                    parts: vec![None; p.parts.min(MAX_PARTS) as usize],
                    declined: too_many
                        .then(|| format!("sent {} parts, more than {MAX_PARTS}", p.parts)),
                });
                self.sources.len() - 1
            }
        };
        let s = &mut self.sources[i];
        if s.declined.is_some() {
            return;
        }
        if let Some(reason) = p.declined {
            s.declined = Some(reason);
            return;
        }
        if self.bytes + p.text.len() > MAX_COLLECTION_BYTES {
            return;
        }
        if let Some(slot) = s.parts.get_mut(p.part as usize) {
            if slot.is_none() {
                self.bytes += p.text.len();
            }
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

    /// Where every source stands, for the page that waits on it. `done`: the query has answered,
    /// so what has not arrived will not.
    pub fn status(&self, now: EpochMs, done: bool) -> RoomDiagnosticsStatus {
        let coordinator = self.sources.iter().find(|s| s.id == COORDINATOR);
        let mut devices: Vec<DiagnosticsSourceStatus> = self
            .asked
            .iter()
            .map(
                |(id, name)| match self.sources.iter().find(|s| s.id == *id) {
                    Some(s) => s.status(done),
                    None => DiagnosticsSourceStatus {
                        id: id.clone(),
                        name: name.clone(),
                        state: if done {
                            DiagnosticsSourceState::NoAnswer
                        } else {
                            DiagnosticsSourceState::Waiting
                        },
                        parts_received: 0,
                        parts: 0,
                        bytes: 0,
                        reason: None,
                    },
                },
            )
            .collect();
        devices.extend(
            self.sources
                .iter()
                .filter(|s| !is_reserved(&s.id) && !self.asked.iter().any(|(id, _)| *id == s.id))
                .map(|s| s.status(done)),
        );
        RoomDiagnosticsStatus {
            request_id: self.request_id.clone(),
            done,
            elapsed_ms: (now - self.started_at).max(0.0),
            unavailable: None,
            room_answered: self.room_answered,
            room_declined: self
                .sources
                .iter()
                .find(|s| s.id == ROOM)
                .and_then(|s| s.declined.clone()),
            coordinator_log: coordinator
                .is_some_and(|c| c.text().contains(COORDINATOR_LOG_HEADING)),
            coordinator: coordinator.map(|c| c.status(done)),
            devices,
        }
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
    fn status_follows_every_source() {
        let mut c = Collection::new("r", 1_000.0);
        let s = c.status(1_000.0, false);
        assert!(!s.room_answered && s.coordinator.is_none() && s.devices.is_empty());
        c.add(part(COORDINATOR, 0, 2, "hocket-coordinator 1\n"));
        let mut room = part(ROOM, 0, 1, "state");
        room.asked = vec![device("b"), device("c"), device("d")];
        c.add(room);
        c.add(part("b", 0, 2, "hello "));
        let mut declined = part("c", 0, 1, "");
        declined.declined = Some("developer mode is off on this device".into());
        c.add(declined);
        let s = c.status(3_000.0, false);
        assert!(s.room_answered && !s.done);
        assert_eq!(s.elapsed_ms, 2_000.0);
        let coord = s.coordinator.as_ref().unwrap();
        assert_eq!(coord.state, DiagnosticsSourceState::Receiving);
        assert_eq!((coord.parts_received, coord.parts), (1, 2));
        assert!(!s.coordinator_log);
        let states: Vec<_> = s.devices.iter().map(|d| (d.id.as_str(), d.state)).collect();
        assert_eq!(
            states,
            vec![
                ("b", DiagnosticsSourceState::Receiving),
                ("c", DiagnosticsSourceState::Declined),
                ("d", DiagnosticsSourceState::Waiting),
            ]
        );
        assert_eq!(s.devices[0].name, "Name b");
        assert_eq!(s.devices[2].name, "Device d");
        assert!(s.devices[1]
            .reason
            .as_deref()
            .unwrap()
            .contains("developer mode"));
        c.add(part(
            COORDINATOR,
            1,
            2,
            &format!("{COORDINATOR_LOG_HEADING}60 minutes)\n"),
        ));
        c.add(part("b", 1, 2, "world"));
        let s = c.status(4_000.0, true);
        assert!(s.coordinator_log);
        assert_eq!(
            s.coordinator.unwrap().state,
            DiagnosticsSourceState::Received
        );
        assert_eq!(s.devices[0].state, DiagnosticsSourceState::Received);
        assert_eq!(s.devices[0].bytes, 11);
        assert_eq!(s.devices[2].state, DiagnosticsSourceState::NoAnswer);
    }

    #[test]
    fn status_carries_the_rooms_refusal() {
        let mut c = Collection::new("r", 0.0);
        let mut room = part(ROOM, 0, 1, "");
        room.declined = Some("asked again too soon; try in a few seconds".into());
        c.add(room);
        assert!(c.is_complete());
        let s = c.status(0.0, true);
        assert!(s.room_answered && s.devices.is_empty() && s.coordinator.is_none());
        assert!(s.room_declined.unwrap().contains("too soon"));
    }

    #[test]
    fn status_marks_partial_answers_incomplete_once_done() {
        let mut c = Collection::new("r", 0.0);
        let mut room = part(ROOM, 0, 1, "state");
        room.asked = vec![device("b")];
        c.add(room);
        c.add(part("b", 0, 3, "a"));
        assert_eq!(
            c.status(0.0, false).devices[0].state,
            DiagnosticsSourceState::Receiving
        );
        assert_eq!(
            c.status(0.0, true).devices[0].state,
            DiagnosticsSourceState::Incomplete
        );
    }

    #[test]
    fn parts_stay_within_the_part_count_and_cap_cuts_near_lines() {
        let control = "\u{1}".repeat(MAX_ANSWER_BYTES);
        let p = parts(&control);
        assert!(p.len() <= MAX_PARTS as usize, "{}", p.len());
        let p = parts(&"x".repeat(3 * MAX_ANSWER_BYTES));
        assert!(p.len() <= MAX_PARTS as usize);
        // One long last line: the tail is kept, not snapped away.
        let text = format!("{}\n{}\n", "a".repeat(10_000), "b".repeat(200_000));
        let capped = cap(&text, 100_000);
        assert!(
            capped.ends_with("bbbb\n"),
            "{}",
            &capped[capped.len() - 20..]
        );
        assert!(capped.len() <= 100_000);
    }

    #[test]
    fn a_source_with_too_many_parts_is_not_complete_text() {
        let mut c = Collection::new("r", 0.0);
        c.add(part(ROOM, 0, 1, "state"));
        c.add(part(COORDINATOR, 0, MAX_PARTS + 1, "x"));
        assert!(c.is_complete());
        assert!(c.render(0.0).contains("more than 64"));
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
        assert!(c.status(0.0, false).room_declined.is_none());
        assert!(c.expired(COLLECT_TIMEOUT_MS));
        let text = c.render(COLLECT_TIMEOUT_MS);
        assert!(text.contains("Device: Device b (b)\n"), "{text}");
        assert!(text.contains("(no answer)"));
        assert!(!text.contains("stray"));
    }
}
