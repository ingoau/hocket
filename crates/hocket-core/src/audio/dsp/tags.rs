//! ReplayGain tag parsing.
//!
//! Three families of tags carry loudness information:
//!
//! - `REPLAYGAIN_TRACK_GAIN` / `_PEAK` / `REPLAYGAIN_ALBUM_*` (Vorbis
//!   comments, APEv2, ID3v2 `TXXX`, MP4 freeform) — `"-6.54 dB"` and a
//!   linear peak.
//! - `iTunNORM` (MP4 freeform `com.apple.iTunes:iTunNORM`) — Sound Check:
//!   hex fields where fields 0/1 are the volume adjustment per channel on a
//!   1000-per-unit scale (`gain_db = -10·log10(v / 1000)`), fields 6/7 the
//!   peak on a 32768 scale.
//! - `R128_TRACK_GAIN` / `R128_ALBUM_GAIN` (Opus) — Q7.8 integers relative
//!   to −23 LUFS; ReplayGain's −18 LUFS reference is 5 dB higher.
//!
//! The pure parsers are always compiled and unit-tested; [`read_replaygain`]
//! (feature `native-audio`) walks a file's metadata with Symphonia and
//! applies them in priority order: explicit ReplayGain, then R128, then
//! iTunNORM, then the LAME tag's radio gain.

use crate::api::ReplayGain;

/// Parse `"-6.54 dB"`, `"+1.2"`, `"−3,5 dB"` style gain strings.
pub fn parse_gain_db(s: &str) -> Option<f64> {
    let t = s
        .trim()
        .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c.is_whitespace())
        .trim();
    let t = t.replace(',', ".").replace('\u{2212}', "-");
    let t = t.trim_start_matches('+');
    let v: f64 = t.parse().ok()?;
    if v.is_finite() && v.abs() <= 60.0 {
        Some(v)
    } else {
        None
    }
}

/// Parse a peak value (`"0.988"`, sometimes with trailing text).
pub fn parse_peak(s: &str) -> Option<f64> {
    let t = s
        .trim()
        .split(|c: char| c.is_whitespace())
        .next()?
        .replace(',', ".");
    let v: f64 = t.parse().ok()?;
    if v.is_finite() && (0.0..=100.0).contains(&v) {
        Some(v)
    } else {
        None
    }
}

/// iTunNORM → (track gain dB, peak). Uses the louder of the two channel
/// adjustments (the smaller gain), as iTunes does.
pub fn parse_itunnorm(s: &str) -> Option<(f64, Option<f64>)> {
    let fields: Vec<u64> = s
        .split_whitespace()
        .filter_map(|f| u64::from_str_radix(f, 16).ok())
        .collect();
    if fields.len() < 2 {
        return None;
    }
    let v = fields[0].max(fields[1]);
    if v == 0 {
        return None;
    }
    let gain = -10.0 * (v as f64 / 1000.0).log10();
    let peak = fields
        .get(6)
        .zip(fields.get(7))
        .map(|(a, b)| (*a.max(b)) as f64 / 32768.0)
        .filter(|p| *p > 0.0);
    Some((gain, peak))
}

/// R128 Q7.8 gain (relative to −23 LUFS) → ReplayGain dB (relative to −18).
pub fn parse_r128_gain(s: &str) -> Option<f64> {
    let v: i32 = s.trim().parse().ok()?;
    Some(f64::from(v) / 256.0 + 5.0)
}

/// Fold one `key = value` tag into `rg`. Keys are matched case-insensitively
/// and with any `com.apple.iTunes:`/`----:` freeform prefix stripped.
/// Returns `true` if the tag was recognised.
pub fn apply_tag(rg: &mut ReplayGain, key: &str, value: &str) -> bool {
    let k = key
        .rsplit(':')
        .next()
        .unwrap_or(key)
        .trim()
        .to_ascii_uppercase();
    match k.as_str() {
        "REPLAYGAIN_TRACK_GAIN" => set_if_none(&mut rg.track_gain_db, parse_gain_db(value)),
        "REPLAYGAIN_TRACK_PEAK" => set_if_none(&mut rg.track_peak, parse_peak(value)),
        "REPLAYGAIN_ALBUM_GAIN" => set_if_none(&mut rg.album_gain_db, parse_gain_db(value)),
        "REPLAYGAIN_ALBUM_PEAK" => set_if_none(&mut rg.album_peak, parse_peak(value)),
        "R128_TRACK_GAIN" => set_if_none(&mut rg.track_gain_db, parse_r128_gain(value)),
        "R128_ALBUM_GAIN" => set_if_none(&mut rg.album_gain_db, parse_r128_gain(value)),
        "ITUNNORM" => match parse_itunnorm(value) {
            Some((g, p)) => {
                let a = set_if_none(&mut rg.track_gain_db, Some(g));
                if let Some(p) = p {
                    set_if_none(&mut rg.track_peak, Some(p));
                }
                a
            }
            None => false,
        },
        _ => false,
    }
}

fn set_if_none(slot: &mut Option<f64>, value: Option<f64>) -> bool {
    match (slot.is_none(), value) {
        (true, Some(v)) => {
            *slot = Some(v);
            true
        }
        (false, Some(_)) => true,
        _ => false,
    }
}

/// Whether the tags carry anything usable.
pub fn has_any(rg: &ReplayGain) -> bool {
    rg.track_gain_db.is_some() || rg.album_gain_db.is_some()
}

/// Read ReplayGain tags from a local file via Symphonia's metadata, falling
/// back to the LAME header for MP3. `Ok(None)` when the file has no
/// loudness tags at all (the caller then computes them).
#[cfg(feature = "native-audio")]
pub fn read_replaygain(
    path: &std::path::Path,
) -> Result<Option<ReplayGain>, crate::audio::native::decoder::DecodeError> {
    use symphonia::core::meta::{StandardTagKey, Value};

    let mut rg = ReplayGain::default();
    let mut opened = crate::audio::native::decoder::open_file(path)?;
    let mut apply_revision = |tags: &[symphonia::core::meta::Tag]| {
        for tag in tags {
            let value = match &tag.value {
                Value::String(s) => s.clone(),
                Value::Float(f) => f.to_string(),
                Value::SignedInt(i) => i.to_string(),
                Value::UnsignedInt(u) => u.to_string(),
                Value::Binary(b) => String::from_utf8_lossy(b).into_owned(),
                _ => continue,
            };
            let key = match tag.std_key {
                Some(StandardTagKey::ReplayGainTrackGain) => "REPLAYGAIN_TRACK_GAIN",
                Some(StandardTagKey::ReplayGainTrackPeak) => "REPLAYGAIN_TRACK_PEAK",
                Some(StandardTagKey::ReplayGainAlbumGain) => "REPLAYGAIN_ALBUM_GAIN",
                Some(StandardTagKey::ReplayGainAlbumPeak) => "REPLAYGAIN_ALBUM_PEAK",
                _ => tag.key.as_str(),
            };
            apply_tag(&mut rg, key, &value);
        }
    };
    if let Some(mut m) = opened.probed_metadata.get() {
        while let Some(rev) = m.current() {
            apply_revision(rev.tags());
            if m.pop().is_none() {
                break;
            }
        }
    }
    let mut m = opened.format.metadata();
    m.skip_to_latest();
    if let Some(rev) = m.current() {
        apply_revision(rev.tags());
    }
    if !has_any(&rg) {
        if let Some(lame) = opened.lame {
            rg.track_gain_db = lame.gain.track_gain_db;
            rg.album_gain_db = lame.gain.album_gain_db;
            rg.track_peak = lame.gain.peak;
        }
    }
    Ok(if has_any(&rg) { Some(rg) } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_strings() {
        assert_eq!(parse_gain_db("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_gain_db("+1.20 dB"), Some(1.2));
        assert_eq!(parse_gain_db(" 3.5"), Some(3.5));
        assert_eq!(parse_gain_db("−2,5 dB"), Some(-2.5));
        assert_eq!(parse_gain_db("dB"), None);
        assert_eq!(parse_gain_db("999 dB"), None);
        assert_eq!(parse_peak("0.988"), Some(0.988));
        assert_eq!(parse_peak("1,05 (clipping)"), Some(1.05));
        assert_eq!(parse_peak("-1"), None);
    }

    #[test]
    fn itunnorm_and_r128() {
        // 1000 → 0 dB; 2000 → −3.01 dB; peak 16384/32768 = 0.5
        let (g, p) = parse_itunnorm(
            " 000003E8 000007D0 00000000 00000000 00000000 00000000 00004000 00002000",
        )
        .unwrap();
        assert!((g + 3.0103).abs() < 1e-3);
        assert_eq!(p, Some(0.5));
        assert_eq!(parse_itunnorm("00000000 00000000").map(|x| x.0), None);
        assert_eq!(parse_itunnorm("junk"), None);
        assert_eq!(parse_r128_gain("-1280"), Some(0.0));
        assert_eq!(parse_r128_gain("0"), Some(5.0));
        assert_eq!(parse_r128_gain("x"), None);
    }

    #[test]
    fn apply_tag_matches_keys_loosely_and_prefers_explicit_values() {
        let mut rg = ReplayGain::default();
        assert!(apply_tag(&mut rg, "replaygain_track_gain", "-5.0 dB"));
        assert!(apply_tag(
            &mut rg,
            "----:com.apple.iTunes:replaygain_track_peak",
            "0.9"
        ));
        assert!(apply_tag(&mut rg, "REPLAYGAIN_ALBUM_GAIN", "-7 dB"));
        assert!(!apply_tag(&mut rg, "TITLE", "x"));
        // iTunNORM does not override an explicit gain.
        assert!(apply_tag(
            &mut rg,
            "com.apple.iTunes:iTunNORM",
            " 000003E8 000003E8"
        ));
        assert_eq!(rg.track_gain_db, Some(-5.0));
        assert_eq!(rg.track_peak, Some(0.9));
        assert_eq!(rg.album_gain_db, Some(-7.0));
        assert!(has_any(&rg));
        let mut only_norm = ReplayGain::default();
        apply_tag(&mut only_norm, "iTunNORM", " 000007D0 000003E8");
        assert!((only_norm.track_gain_db.unwrap() + 3.0103).abs() < 1e-3);
        assert!(!apply_tag(
            &mut ReplayGain::default(),
            "REPLAYGAIN_TRACK_GAIN",
            "garbage"
        ));
    }
}
