//! Formats a particular Cast receiver can't play although it says it can
//! (the Pixel Tablet's receiver claims FLAC, then crashes right after
//! `play()`), so the remote path transcodes them for that receiver only
//! and stays lossless everywhere else.
//!
//! Nothing is known up front: a receiver whose session dies right after it
//! started an untranscoded track gets that track's format noted here,
//! kept in `saved_state` by receiver id.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::api::{Renderer, Track};

/// `saved_state` key of [`Learned`].
pub const STATE_KEY: &str = "renderer_formats";

/// A format a receiver failed on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub suffix: String,
    /// Only failed on hi-res files of it so far (see [`hi_res`]).
    pub hi_res_only: bool,
}

/// What this device has learned, by receiver id.
pub type Learned = HashMap<String, Vec<Format>>;

/// More than 16 bits or 48 kHz.
pub fn hi_res(track: &Track) -> bool {
    track.bit_depth.is_some_and(|b| b > 16) || track.sample_rate.is_some_and(|r| r > 48_000)
}

/// Whether `renderer` can't play `track` as it is (and so it must be
/// transcoded on it).
pub fn cannot_play(renderer: &Renderer, learned: &Learned, track: &Track) -> bool {
    let Some(suffix) = track.suffix.as_deref() else {
        return false;
    };
    learned.get(&renderer.id).is_some_and(|formats| {
        formats
            .iter()
            .any(|f| f.suffix.eq_ignore_ascii_case(suffix) && (!f.hi_res_only || hi_res(track)))
    })
}

/// Note that `renderer` failed on `track`. Returns whether that changed
/// anything: a hi-res file marks only hi-res files of its format (until a
/// plain one fails too), a plain one the whole format.
pub fn learn(learned: &mut Learned, renderer: &Renderer, track: &Track) -> bool {
    let Some(suffix) = track.suffix.as_deref().filter(|s| !s.is_empty()) else {
        return false;
    };
    let hi_res_only = hi_res(track);
    let formats = learned.entry(renderer.id.clone()).or_default();
    match formats
        .iter_mut()
        .find(|f| f.suffix.eq_ignore_ascii_case(suffix))
    {
        Some(f) if f.hi_res_only && !hi_res_only => {
            f.hi_res_only = false;
            true
        }
        Some(_) => false,
        None => {
            formats.push(Format {
                suffix: suffix.to_ascii_lowercase(),
                hi_res_only,
            });
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer(id: &str) -> Renderer {
        Renderer {
            id: id.into(),
            name: "Living room".into(),
        }
    }

    fn track(suffix: &str, bit_depth: u32, sample_rate: u32) -> Track {
        Track {
            suffix: Some(suffix.into()),
            bit_depth: Some(bit_depth),
            sample_rate: Some(sample_rate),
            ..Default::default()
        }
    }

    #[test]
    fn a_receiver_plays_everything_until_it_fails() {
        let learned = Learned::new();
        assert!(!cannot_play(
            &renderer("tablet"),
            &learned,
            &track("flac", 24, 96_000)
        ));
    }

    #[test]
    fn learning_starts_with_hi_res_and_widens_to_the_format() {
        let mut learned = Learned::new();
        let tv = renderer("tv");
        let other = renderer("other");
        assert!(learn(&mut learned, &tv, &track("flac", 24, 48_000)));
        assert!(
            !learn(&mut learned, &tv, &track("flac", 16, 96_000)),
            "already known"
        );
        assert!(cannot_play(&tv, &learned, &track("flac", 16, 96_000)));
        assert!(
            !cannot_play(&tv, &learned, &track("flac", 16, 44_100)),
            "plain FLAC still lossless"
        );
        assert!(!cannot_play(&tv, &learned, &track("mp3", 24, 96_000)));
        assert!(
            !cannot_play(&other, &learned, &track("flac", 24, 48_000)),
            "per receiver"
        );
        assert!(learn(&mut learned, &tv, &track("FLAC", 16, 44_100)));
        assert!(cannot_play(&tv, &learned, &track("flac", 16, 44_100)));
        assert!(
            !learn(&mut learned, &tv, &track("flac", 24, 96_000)),
            "no narrowing back"
        );
        assert!(
            !learn(&mut learned, &tv, &Track::default()),
            "no suffix, nothing to learn"
        );
        assert_eq!(learned["tv"].len(), 1);
    }
}
