//! Platform decode-capability lists.
//!
//! [`crate::api::TranscodingProfile::cannot_decode`] carries the container /
//! codec suffixes a device cannot play natively so the core forces a
//! transcode for them regardless of the bitrate preference. This module is
//! where those defaults come from. Suffixes are the lower-case file suffixes
//! Navidrome reports in `Track.suffix`.
//!
//! ## Desktop (Symphonia 0.5.5, feature `native-audio`)
//!
//! The pinned Symphonia provides, with the `all` feature set enabled in the
//! workspace: MP1/2/3, AAC (LC, in MP4/M4A and ADTS), FLAC, Ogg Vorbis, PCM
//! (WAV, AIFF, CAF), ADPCM, ALAC (M4A/CAF) and Matroska/WebM demuxing. It has
//! **no Opus, WavPack, APE, DSD or Musepack decoder** in 0.5.x — Opus is
//! demuxed from Ogg but not decoded. The design note's "ALAC and WavPack
//! behind feature flags" is half true for this version: ALAC is in, WavPack
//! is not, so the desktop list is APE, DSD, WavPack, Opus, Musepack, and the
//! rare lossy codecs Symphonia doesn't ship (WMA, AC-3/E-AC-3, DTS, TrueHD).
//!
//! ## Android (Media3 / ExoPlayer, platform decoders)
//!
//! Per <https://developer.android.com/media/media3/exoplayer/supported-formats>:
//! MP4/M4A, Matroska, MP3, Ogg (Vorbis, Opus, FLAC), WAV, ADTS, FLAC (API 27+
//! hardware, all levels via the FLAC extension) and AMR are supported.
//! **APE, WavPack and DSD are not**, and ALAC only via the optional FFmpeg
//! extension, which the app doesn't bundle, so ALAC is on the list too.

use crate::api::Platform;

/// Suffixes desktop (Symphonia) cannot decode natively.
pub const DESKTOP_CANNOT_DECODE: &[&str] = &[
    "ape", "dsf", "dff", "wv", "opus", "mpc", "wma", "ac3", "eac3", "dts", "thd", "tak", "ofr", "shn", "tta",
];

/// Suffixes Android (ExoPlayer with platform decoders) cannot decode natively.
pub const ANDROID_CANNOT_DECODE: &[&str] = &[
    "ape", "dsf", "dff", "wv", "alac", "mpc", "wma", "ac3", "eac3", "dts", "thd", "tak", "ofr", "shn", "tta",
];

/// Suffixes desktop decodes natively (informational; used by the UI to label
/// "plays without transcoding").
pub const DESKTOP_NATIVE: &[&str] = &["mp3", "mp2", "mp1", "aac", "m4a", "mp4", "flac", "ogg", "oga", "wav", "aif", "aiff", "aifc", "caf", "alac", "mka", "webm"];

/// Suffixes Android decodes natively.
pub const ANDROID_NATIVE: &[&str] = &["mp3", "aac", "m4a", "mp4", "flac", "ogg", "oga", "opus", "wav", "mka", "webm", "amr", "3gp"];

/// The default `cannot_decode` list used to seed a
/// [`crate::api::TranscodingProfile`] for `platform`. The coordinator never
/// plays audio and gets an empty list.
pub fn default_cannot_decode(platform: Platform) -> Vec<String> {
    let list: &[&str] = match platform {
        Platform::Android => ANDROID_CANNOT_DECODE,
        Platform::Linux | Platform::MacOs | Platform::Windows => DESKTOP_CANNOT_DECODE,
        Platform::Coordinator => &[],
    };
    list.iter().map(|s| s.to_string()).collect()
}

/// Whether a track with `suffix` needs a transcode on `platform` given the
/// profile's `cannot_decode` list (case-insensitive).
pub fn needs_transcode(suffix: Option<&str>, cannot_decode: &[String]) -> bool {
    match suffix {
        Some(s) => {
            let s = s.trim_start_matches('.').to_ascii_lowercase();
            cannot_decode.iter().any(|c| c.eq_ignore_ascii_case(&s))
        }
        None => false,
    }
}

/// Whether `suffix` is in the native list for `platform`.
pub fn decodes_natively(platform: Platform, suffix: &str) -> bool {
    let list: &[&str] = match platform {
        Platform::Android => ANDROID_NATIVE,
        Platform::Linux | Platform::MacOs | Platform::Windows => DESKTOP_NATIVE,
        Platform::Coordinator => &[],
    };
    let s = suffix.trim_start_matches('.').to_ascii_lowercase();
    list.iter().any(|n| *n == s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_list_is_roughly_ape_and_dsd_plus_what_symphonia_lacks() {
        let d = default_cannot_decode(Platform::Linux);
        for s in ["ape", "dsf", "dff", "wv", "opus"] {
            assert!(d.contains(&s.to_string()), "{s}");
        }
        for s in ["flac", "mp3", "m4a", "alac", "ogg", "wav", "aiff"] {
            assert!(!d.contains(&s.to_string()), "{s} should decode natively");
            assert!(decodes_natively(Platform::Windows, s), "{s}");
        }
    }

    #[test]
    fn android_list_follows_exoplayer() {
        let a = default_cannot_decode(Platform::Android);
        for s in ["ape", "wv", "dsf", "alac"] {
            assert!(a.contains(&s.to_string()), "{s}");
        }
        for s in ["opus", "flac", "mp3", "m4a"] {
            assert!(!a.contains(&s.to_string()), "{s}");
            assert!(decodes_natively(Platform::Android, s));
        }
    }

    #[test]
    fn coordinator_has_no_opinion() {
        assert!(default_cannot_decode(Platform::Coordinator).is_empty());
        assert!(!decodes_natively(Platform::Coordinator, "mp3"));
    }

    #[test]
    fn needs_transcode_is_case_insensitive_and_tolerates_dots() {
        let list = default_cannot_decode(Platform::MacOs);
        assert!(needs_transcode(Some("APE"), &list));
        assert!(needs_transcode(Some(".wv"), &list));
        assert!(!needs_transcode(Some("flac"), &list));
        assert!(!needs_transcode(None, &list));
    }
}
