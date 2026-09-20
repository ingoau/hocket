//! Keyboard chord normalisation.
//!
//! Canonical form: modifiers in `Ctrl`, `Alt`, `Shift`, then the platform's
//! command modifier (`Cmd` on macOS, `Meta` elsewhere), joined with `+`, then
//! one key (`K`, `Space`, `ArrowLeft`, `F5`, `Delete`, …). `Mod` in input means
//! the primary modifier: `Cmd` on macOS, `Ctrl` elsewhere. [`to_portable`]
//! writes the primary modifier back as `Mod`, which is what gets persisted so
//! a config travels between platforms.

use crate::api::Platform;

#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum ChordError {
    #[error("empty chord")]
    Empty,
    #[error("unknown key {0}")]
    UnknownKey(String),
    #[error("a chord needs exactly one non-modifier key")]
    KeyCount,
}

/// The primary modifier as rendered on this platform.
pub fn primary_modifier(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => "Cmd",
        _ => "Ctrl",
    }
}

/// The command/meta modifier name on this platform.
fn meta_name(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => "Cmd",
        _ => "Meta",
    }
}

/// Normalise any reasonable spelling ("ctrl+shift+z", "Mod+K", "cmd + f",
/// "Ctrl++") into the canonical concrete chord for `platform`.
pub fn normalise(input: &str, platform: Platform) -> Result<String, ChordError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ChordError::Empty);
    }
    // "Ctrl++" means the plus key.
    let mut tokens: Vec<String> = trimmed.split('+').map(|t| t.trim().to_string()).collect();
    let mut plus_key = false;
    if tokens.len() >= 2 && tokens.last().map(|t| t.is_empty()).unwrap_or(false) {
        plus_key = true;
        while tokens.last().map(|t| t.is_empty()).unwrap_or(false) {
            tokens.pop();
        }
    }
    let (mut ctrl, mut alt, mut shift, mut meta) = (false, false, false, false);
    let mut key: Option<String> = None;
    let set_key = |k: String, key: &mut Option<String>| -> Result<(), ChordError> {
        if key.is_some() {
            return Err(ChordError::KeyCount);
        }
        *key = Some(k);
        Ok(())
    };
    for t in tokens.iter().filter(|t| !t.is_empty()) {
        match t.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            "cmd" | "command" | "meta" | "super" | "win" | "windows" => meta = true,
            "mod" | "cmdorctrl" | "commandorcontrol" => {
                if platform == Platform::MacOs {
                    meta = true
                } else {
                    ctrl = true
                }
            }
            other => set_key(
                key_name(other).ok_or_else(|| ChordError::UnknownKey(t.clone()))?,
                &mut key,
            )?,
        }
    }
    if plus_key {
        set_key("Plus".into(), &mut key)?;
    }
    let key = key.ok_or(ChordError::KeyCount)?;
    let mut parts: Vec<&str> = vec![];
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if meta {
        parts.push(meta_name(platform));
    }
    parts.push(&key);
    Ok(parts.join("+"))
}

/// Replace the primary modifier with `Mod` for persistence.
pub fn to_portable(chord: &str, platform: Platform) -> String {
    let primary = primary_modifier(platform);
    chord
        .split('+')
        .map(|t| if t == primary { "Mod" } else { t })
        .collect::<Vec<_>>()
        .join("+")
}

fn key_name(lower: &str) -> Option<String> {
    let named = match lower {
        "space" | "spacebar" | " " => "Space",
        "enter" | "return" => "Enter",
        "esc" | "escape" => "Escape",
        "tab" => "Tab",
        "del" | "delete" => "Delete",
        "backspace" => "Backspace",
        "left" | "arrowleft" => "ArrowLeft",
        "right" | "arrowright" => "ArrowRight",
        "up" | "arrowup" => "ArrowUp",
        "down" | "arrowdown" => "ArrowDown",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "insert" => "Insert",
        "plus" => "Plus",
        "minus" | "-" => "Minus",
        "comma" | "," => "Comma",
        "period" | "." => "Period",
        "slash" | "/" => "Slash",
        "backslash" | "\\" => "Backslash",
        "semicolon" | ";" => "Semicolon",
        "quote" | "'" => "Quote",
        "backquote" | "`" => "Backquote",
        "bracketleft" | "[" => "BracketLeft",
        "bracketright" | "]" => "BracketRight",
        "equal" | "=" => "Equal",
        "mediaplaypause" => "MediaPlayPause",
        "medianext" | "mediatracknext" => "MediaTrackNext",
        "mediaprevious" | "mediatrackprevious" => "MediaTrackPrevious",
        "mediastop" => "MediaStop",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.into());
    }
    if lower.len() == 1 && lower.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Some(lower.to_ascii_uppercase());
    }
    if let Some(n) = lower.strip_prefix('f') {
        if let Ok(n) = n.parse::<u32>() {
            if (1..=24).contains(&n) {
                return Some(format!("F{n}"));
            }
        }
    }
    if let Some(n) = lower.strip_prefix("numpad") {
        if n.len() == 1 && n.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("Numpad{n}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_spelling_and_order() {
        assert_eq!(
            normalise("shift+ctrl+z", Platform::Linux).unwrap(),
            "Ctrl+Shift+Z"
        );
        assert_eq!(
            normalise(" Ctrl + Shift + z ", Platform::Windows).unwrap(),
            "Ctrl+Shift+Z"
        );
        assert_eq!(normalise("Mod+K", Platform::Linux).unwrap(), "Ctrl+K");
        assert_eq!(normalise("Mod+K", Platform::MacOs).unwrap(), "Cmd+K");
        assert_eq!(normalise("cmd+k", Platform::MacOs).unwrap(), "Cmd+K");
        assert_eq!(normalise("cmd+k", Platform::Linux).unwrap(), "Meta+K");
        assert_eq!(
            normalise("ctrl+k", Platform::MacOs).unwrap(),
            "Ctrl+K",
            "a real Control key stays Control on macOS"
        );
        assert_eq!(
            normalise("alt+shift+ctrl+meta+left", Platform::Linux).unwrap(),
            "Ctrl+Alt+Shift+Meta+ArrowLeft"
        );
        assert_eq!(normalise("option+f5", Platform::MacOs).unwrap(), "Alt+F5");
        assert_eq!(normalise("space", Platform::Linux).unwrap(), "Space");
        assert_eq!(normalise("Ctrl++", Platform::Linux).unwrap(), "Ctrl+Plus");
        assert_eq!(normalise("5", Platform::Linux).unwrap(), "5");
        assert_eq!(normalise("Delete", Platform::Linux).unwrap(), "Delete");
    }

    #[test]
    fn rejects_bad_chords() {
        assert_eq!(normalise("", Platform::Linux), Err(ChordError::Empty));
        assert_eq!(
            normalise("ctrl+shift", Platform::Linux),
            Err(ChordError::KeyCount)
        );
        assert_eq!(normalise("a+b", Platform::Linux), Err(ChordError::KeyCount));
        assert_eq!(
            normalise("ctrl+banana", Platform::Linux),
            Err(ChordError::UnknownKey("banana".into()))
        );
        assert_eq!(
            normalise("f99", Platform::Linux),
            Err(ChordError::UnknownKey("f99".into()))
        );
    }

    #[test]
    fn portable_round_trip() {
        let mac = normalise("Mod+Shift+Z", Platform::MacOs).unwrap();
        assert_eq!(
            mac, "Shift+Cmd+Z",
            "canonical order is Ctrl, Alt, Shift, Cmd"
        );
        assert_eq!(to_portable(&mac, Platform::MacOs), "Shift+Mod+Z");
        let linux = normalise("Mod+Shift+Z", Platform::Linux).unwrap();
        assert_eq!(linux, "Ctrl+Shift+Z");
        assert_eq!(to_portable(&linux, Platform::Linux), "Mod+Shift+Z");
        // A macOS Control chord is not the primary modifier and stays literal.
        assert_eq!(to_portable("Ctrl+K", Platform::MacOs), "Ctrl+K");
        // And back on the other platform.
        assert_eq!(
            normalise(&to_portable(&mac, Platform::MacOs), Platform::Windows).unwrap(),
            "Ctrl+Shift+Z"
        );
        assert_eq!(normalise("Shift+Mod+Z", Platform::MacOs).unwrap(), mac);
    }
}
