//! Listening stats from local play history, and the rating→love bridge.
//! Pure functions over rows the actor reads from `play_history` joined with
//! the mirror.
//!
//! Entry points for the actor:
//!
//! | Need | Call |
//! |---|---|
//! | `Query::Stats { period_days }` | [`listening_stats`] `(&[PlayRow], &StatsOptions, now_ms) → api::ListeningStats` (plus [`StatsExtras`] with streaks via [`listening_stats_with_extras`]) |
//! | `Query::RecentlyPlayed { limit }` | [`recently_played`] `(&[PlayRow], limit) → Vec<api::PlayHistoryEntry>` |
//! | Streaks on their own | [`streaks`] |
//! | Rating→love bridge on `SetRating` | [`bridge_love`] `(rating, threshold, enabled) → Option<bool>` |
//!
//! Hour/weekday buckets are in the device's local time: pass the zone
//! offset (minutes east of UTC) in [`StatsOptions`]. A "play" is any
//! history row whose `played_ms` reaches the same threshold the scrobbler
//! records plays at ([`is_play`], 50% of the track or 4 minutes): skips
//! don't count, but still appear in "recently played".

use std::collections::HashMap;

use crate::api::{Album, Artist, ListeningStats, PlayHistoryEntry, TrackSummary};
use crate::filters::dates::{hour_of_day, local_date, CivilDate, MS_PER_DAY};

/// One `play_history` row joined with what the mirror knows about the track.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayRow {
    pub track: TrackSummary,
    /// When the play *started*, epoch ms.
    pub played_at: f64,
    /// How much of it was actually heard, ms.
    pub played_ms: u32,
    pub scrobbled: bool,
    pub device_id: String,
    /// Album details when the mirror has them (for top albums).
    pub album: Option<Album>,
    /// Artist details when the mirror has them (for top artists).
    pub artist: Option<Artist>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatsOptions {
    /// Length of the reporting period ending at `now`.
    pub period_days: u32,
    /// Minutes east of UTC for hour/weekday/streak bucketing.
    pub tz_offset_minutes: i32,
    /// How many entries in each top list.
    pub top_n: usize,
}

impl Default for StatsOptions {
    fn default() -> Self {
        Self {
            period_days: 30,
            tz_offset_minutes: 0,
            top_n: 10,
        }
    }
}

/// Whether a history row is a *play* for counting: [`PlayRow::played_ms`]
/// must reach the same threshold the recorder stores rows at
/// ([`crate::outbox::scrobbler::threshold_ms`], 50% of the track or
/// 4 minutes; tracks under 30 s never count). Relative, so a fully heard
/// short track counts — the row only carries the time heard when the
/// threshold was crossed, not the whole track.
pub fn is_play(row: &PlayRow) -> bool {
    use crate::outbox::scrobbler::{threshold_ms, MIN_TRACK_MS};
    match row.track.duration_ms {
        0 => row.played_ms >= MIN_TRACK_MS,
        d if d < MIN_TRACK_MS => false,
        d => row.played_ms >= threshold_ms(d),
    }
}

/// Streaks of consecutive local days with at least one play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Streaks {
    /// Days in a row ending today or yesterday (a day without plays so far
    /// today doesn't break it).
    pub current_days: u32,
    /// Longest run in the rows given.
    pub longest_days: u32,
    /// Distinct days with a play.
    pub active_days: u32,
}

/// Things `api::ListeningStats` doesn't carry (yet).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatsExtras {
    pub streaks: Streaks,
    /// Distinct tracks / albums / artists played in the period.
    pub distinct_tracks: u32,
    pub distinct_albums: u32,
    pub distinct_artists: u32,
    /// Plays that were scrobbled.
    pub scrobbled_plays: u32,
    /// Play counts per track id, for the UI's "N plays" beside top lists.
    pub track_play_counts: HashMap<String, u32>,
}

/// Stats for the period `[now − period_days, now]`.
pub fn listening_stats(rows: &[PlayRow], options: &StatsOptions, now_ms: f64) -> ListeningStats {
    listening_stats_with_extras(rows, options, now_ms).0
}

/// Same as [`listening_stats`] plus [`StatsExtras`].
pub fn listening_stats_with_extras(
    rows: &[PlayRow],
    options: &StatsOptions,
    now_ms: f64,
) -> (ListeningStats, StatsExtras) {
    let since = now_ms - f64::from(options.period_days) * MS_PER_DAY;
    let in_period: Vec<&PlayRow> = rows
        .iter()
        .filter(|r| r.played_at >= since && r.played_at <= now_ms && is_play(r))
        .collect();

    let mut stats = ListeningStats {
        period_days: options.period_days,
        plays_by_hour: vec![0; 24],
        plays_by_weekday: vec![0; 7],
        ..Default::default()
    };
    let mut extras = StatsExtras::default();

    // (count, total_ms, first-seen order) per key, so ties break by first play.
    let mut tracks: HashMap<&str, (u32, f64, usize, &TrackSummary)> = HashMap::new();
    let mut albums: HashMap<&str, (u32, f64, usize, &Album)> = HashMap::new();
    let mut artists: HashMap<&str, (u32, f64, usize, &Artist)> = HashMap::new();

    for (i, r) in in_period.iter().enumerate() {
        stats.total_plays += 1;
        stats.total_ms += f64::from(r.played_ms);
        if r.scrobbled {
            extras.scrobbled_plays += 1;
        }
        stats.plays_by_hour[hour_of_day(r.played_at, options.tz_offset_minutes) as usize] += 1;
        stats.plays_by_weekday
            [local_date(r.played_at, options.tz_offset_minutes).weekday_monday0() as usize] += 1;

        let t = tracks
            .entry(r.track.id.as_str())
            .or_insert((0, 0.0, i, &r.track));
        t.0 += 1;
        t.1 += f64::from(r.played_ms);
        if let Some(a) = &r.album {
            let e = albums.entry(a.id.as_str()).or_insert((0, 0.0, i, a));
            e.0 += 1;
            e.1 += f64::from(r.played_ms);
        }
        if let Some(a) = &r.artist {
            let e = artists.entry(a.id.as_str()).or_insert((0, 0.0, i, a));
            e.0 += 1;
            e.1 += f64::from(r.played_ms);
        }
    }

    extras.distinct_tracks = tracks.len() as u32;
    extras.distinct_albums = albums.len() as u32;
    extras.distinct_artists = artists.len() as u32;
    extras.track_play_counts = tracks
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.0))
        .collect();

    stats.top_tracks = top_n(tracks.into_values(), options.top_n)
        .into_iter()
        .cloned()
        .collect();
    stats.top_albums = top_n(albums.into_values(), options.top_n)
        .into_iter()
        .cloned()
        .collect();
    stats.top_artists = top_n(artists.into_values(), options.top_n)
        .into_iter()
        .cloned()
        .collect();
    extras.streaks = streaks(rows, options, now_ms);
    (stats, extras)
}

/// Most plays first, then most time, then first played.
fn top_n<T>(entries: impl Iterator<Item = (u32, f64, usize, T)>, n: usize) -> Vec<T> {
    let mut v: Vec<(u32, f64, usize, T)> = entries.collect();
    v.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.2.cmp(&b.2))
    });
    v.into_iter().take(n).map(|e| e.3).collect()
}

/// Newest first, at most `limit`. Every row counts here, including skips.
pub fn recently_played(rows: &[PlayRow], limit: u32) -> Vec<PlayHistoryEntry> {
    let mut sorted: Vec<&PlayRow> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        b.played_at
            .partial_cmp(&a.played_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted
        .into_iter()
        .take(limit as usize)
        .map(|r| PlayHistoryEntry {
            track: r.track.clone(),
            played_at: r.played_at,
            played_ms: r.played_ms,
            scrobbled: r.scrobbled,
            device_id: r.device_id.clone(),
        })
        .collect()
}

/// Day streaks over *all* rows given (not just the period), in local time.
pub fn streaks(rows: &[PlayRow], options: &StatsOptions, now_ms: f64) -> Streaks {
    let mut days: Vec<i64> = rows
        .iter()
        .filter(|r| is_play(r) && r.played_at <= now_ms)
        .map(|r| local_date(r.played_at, options.tz_offset_minutes).to_days())
        .collect();
    days.sort_unstable();
    days.dedup();
    let active_days = days.len() as u32;
    let mut longest = 0u32;
    let mut run = 0u32;
    let mut prev: Option<i64> = None;
    for d in &days {
        run = match prev {
            Some(p) if *d == p + 1 => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        prev = Some(*d);
    }
    let today = local_date(now_ms, options.tz_offset_minutes).to_days();
    let current_days = match days.last() {
        Some(&last) if last == today || last == today - 1 => run,
        _ => 0,
    };
    Streaks {
        current_days,
        longest_days: longest,
        active_days,
    }
}

/// The one-way rating→love bridge. Returns `Some(true)` when the new rating
/// reaches the threshold and the bridge is on; never `Some(false)`, because
/// lowering a rating must not unlove (the design keeps stars and loves
/// independent apart from this nudge). `None` means "leave loved alone".
pub fn bridge_love(rating: u32, threshold: u32, enabled: bool) -> Option<bool> {
    if enabled && rating > 0 && threshold > 0 && rating >= threshold {
        Some(true)
    } else {
        None
    }
}

/// Local calendar date of a play, for grouping history views by day.
pub fn play_date(row: &PlayRow, tz_offset_minutes: i32) -> CivilDate {
    local_date(row.played_at, tz_offset_minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: f64 = MS_PER_DAY;
    // 2024-01-08T12:00:00Z, a Monday.
    const NOW: f64 = 1_704_715_200_000.0;

    fn row(track: &str, artist: &str, album: &str, at: f64, played_ms: u32) -> PlayRow {
        row_dur(track, artist, album, at, 240_000, played_ms)
    }

    fn row_dur(
        track: &str,
        artist: &str,
        album: &str,
        at: f64,
        duration_ms: u32,
        played_ms: u32,
    ) -> PlayRow {
        PlayRow {
            track: TrackSummary {
                id: track.into(),
                title: track.to_uppercase(),
                artist: Some(artist.into()),
                album: Some(album.into()),
                duration_ms,
                ..Default::default()
            },
            played_at: at,
            played_ms,
            // What the recorder stores: the row carries the flag the session
            // judged, here simply "reached the scrobble threshold".
            scrobbled: played_ms >= crate::outbox::scrobbler::threshold_ms(duration_ms),
            device_id: "dev".into(),
            album: Some(Album {
                id: format!("al-{album}"),
                name: album.into(),
                ..Default::default()
            }),
            artist: Some(Artist {
                id: format!("ar-{artist}"),
                name: artist.into(),
                ..Default::default()
            }),
        }
    }

    fn rows() -> Vec<PlayRow> {
        vec![
            row("t1", "A", "X", NOW - 1.0 * DAY, 240_000),
            row("t1", "A", "X", NOW - 2.0 * DAY, 240_000),
            row("t1", "A", "X", NOW - 3.0 * DAY, 240_000),
            row("t2", "A", "Y", NOW - 1.0 * DAY + 3_600_000.0, 200_000),
            row("t2", "A", "Y", NOW - 5.0 * DAY, 200_000),
            row("t3", "B", "Z", NOW - 6.0 * DAY, 100_000), // 42% heard: below the 50% threshold
            row("t4", "C", "W", NOW - 6.0 * DAY, 5_000),   // skip: below any threshold
            row("t5", "D", "V", NOW - 40.0 * DAY, 240_000), // outside a 30-day period
            row("t6", "E", "U", NOW + DAY, 240_000),       // future clock skew: ignored
        ]
    }

    #[test]
    fn totals_and_top_lists() {
        let (s, extras) = listening_stats_with_extras(&rows(), &StatsOptions::default(), NOW);
        assert_eq!(s.period_days, 30);
        assert_eq!(s.total_plays, 5);
        assert_eq!(s.total_ms, 3.0 * 240_000.0 + 2.0 * 200_000.0);
        assert_eq!(
            s.top_tracks
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["t1", "t2"]
        );
        assert_eq!(
            s.top_albums
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            vec!["X", "Y"]
        );
        assert_eq!(
            s.top_artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            vec!["A"]
        );
        assert_eq!(extras.distinct_tracks, 2);
        assert_eq!(extras.distinct_albums, 2);
        assert_eq!(extras.distinct_artists, 1);
        assert_eq!(extras.scrobbled_plays, 5);
        assert_eq!(extras.track_play_counts["t1"], 3);
        assert_eq!(s.plays_by_hour.len(), 24);
        assert_eq!(s.plays_by_weekday.len(), 7);
        assert_eq!(s.plays_by_hour.iter().sum::<u32>(), 5);
        assert_eq!(s.plays_by_weekday.iter().sum::<u32>(), 5);
    }

    #[test]
    fn buckets_follow_local_time() {
        // Plays at 12:00Z on Sun, Sat, Fri… (NOW is Monday 12:00Z; the
        // partial t3 play at Tue doesn't count).
        let utc = StatsOptions::default();
        let s = listening_stats(&rows(), &utc, NOW);
        assert_eq!(s.plays_by_hour[12], 4);
        assert_eq!(s.plays_by_hour[13], 1); // t2 at 13:00Z
                                            // Monday-first weekdays: Sun(6)=t1+t2, Sat(5)=t1, Fri(4)=t1, Wed(2)=t2.
        assert_eq!(s.plays_by_weekday, vec![0, 0, 1, 0, 1, 1, 2]);

        // UTC+13 pushes 12:00Z to 01:00 the next local day.
        let nz = StatsOptions {
            tz_offset_minutes: 13 * 60,
            ..Default::default()
        };
        let s = listening_stats(&rows(), &nz, NOW);
        assert_eq!(s.plays_by_hour[1], 4);
        assert_eq!(s.plays_by_hour[2], 1);
        // Every play moves one weekday forward: Mon=2, Thu=1, Sat=1, Sun=1.
        assert_eq!(s.plays_by_weekday, vec![2, 0, 0, 1, 0, 1, 1]);
    }

    #[test]
    fn period_is_honoured() {
        let s = listening_stats(
            &rows(),
            &StatsOptions {
                period_days: 2,
                ..Default::default()
            },
            NOW,
        );
        assert_eq!(s.total_plays, 3); // t1 ×2 (1d, 2d) + t2 (1d)
        let s = listening_stats(
            &rows(),
            &StatsOptions {
                period_days: 365,
                ..Default::default()
            },
            NOW,
        );
        assert_eq!(s.total_plays, 6); // t4 (skip) and t3 (42% heard) still don't count
        let s = listening_stats(&[], &StatsOptions::default(), NOW);
        assert_eq!(s.total_plays, 0);
        assert!(s.top_tracks.is_empty());
        assert_eq!(s.plays_by_hour, vec![0; 24]);
    }

    #[test]
    fn play_threshold_matches_the_scrobbler() {
        // A fully heard short track: the row only carries the time played
        // when the threshold was crossed (15 s for a 30 s track) — it must
        // still count, this was the "Listening is empty" bug.
        let short = vec![row_dur("s", "A", "X", NOW - DAY, 30_000, 15_000)];
        assert_eq!(
            listening_stats(&short, &StatsOptions::default(), NOW).total_plays,
            1
        );
        assert_eq!(short[0].scrobbled, true);

        // A 10% partial play of a 5 min track is not a play.
        let partial = vec![row_dur("p", "A", "X", NOW - DAY, 300_000, 30_000)];
        assert_eq!(
            listening_stats(&partial, &StatsOptions::default(), NOW).total_plays,
            0
        );
        // …while crossing its (relative) half does count.
        let half = vec![row_dur("h", "A", "X", NOW - DAY, 300_000, 150_000)];
        assert_eq!(
            listening_stats(&half, &StatsOptions::default(), NOW).total_plays,
            1
        );

        // Long track: the 4-minute cap means 4 min played counts whatever
        // the track's length.
        let long = vec![row_dur("l", "A", "X", NOW - DAY, 3_600_000, 240_000)];
        assert_eq!(
            listening_stats(&long, &StatsOptions::default(), NOW).total_plays,
            1
        );

        // Tracks under 30 s are never plays, even fully heard (the
        // recorder never writes these, but be defensive).
        let tiny = vec![row_dur("t", "A", "X", NOW - DAY, 20_000, 20_000)];
        assert_eq!(
            listening_stats(&tiny, &StatsOptions::default(), NOW).total_plays,
            0
        );

        // Unknown duration (track gone from the mirror): fall back to the
        // absolute scrobble floor.
        let unknown = vec![row_dur("u", "A", "X", NOW - DAY, 0, 15_000)];
        assert_eq!(
            listening_stats(&unknown, &StatsOptions::default(), NOW).total_plays,
            0
        );
        let unknown_ok = vec![row_dur("u2", "A", "X", NOW - DAY, 0, 30_000)];
        assert_eq!(
            listening_stats(&unknown_ok, &StatsOptions::default(), NOW).total_plays,
            1
        );
    }

    #[test]
    fn top_lists_are_capped_and_tie_broken() {
        let mut r = Vec::new();
        for i in 0..15 {
            r.push(row_dur(
                &format!("t{i}"),
                "A",
                "X",
                NOW - DAY,
                240_000,
                130_000 + i * 1000,
            ));
        }
        r.push(row("t0", "A", "X", NOW - DAY, 130_000));
        let s = listening_stats(
            &r,
            &StatsOptions {
                top_n: 3,
                ..Default::default()
            },
            NOW,
        );
        // t0 has two plays; among single plays the longest listened wins.
        assert_eq!(
            s.top_tracks
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["t0", "t14", "t13"]
        );
        assert_eq!(s.top_albums.len(), 1);
    }

    #[test]
    fn recently_played_is_newest_first_and_includes_skips() {
        let h = recently_played(&rows(), 4);
        assert_eq!(
            h.iter().map(|e| e.track.id.as_str()).collect::<Vec<_>>(),
            vec!["t6", "t2", "t1", "t1"]
        );
        assert_eq!(h[1].played_ms, 200_000);
        assert!(h[1].scrobbled);
        assert_eq!(h[1].device_id, "dev");
        assert!(recently_played(&rows(), 0).is_empty());
        assert!(recently_played(&rows(), 100)
            .iter()
            .any(|e| e.track.id == "t4"));
    }

    #[test]
    fn streak_maths() {
        let opts = StatsOptions::default();
        // Days with plays: -1, -2, -3, -5, -40 → current 3 (ends yesterday),
        // longest 3, active 5 (t3's partial play and t4's skip don't count).
        assert_eq!(
            streaks(&rows(), &opts, NOW),
            Streaks {
                current_days: 3,
                longest_days: 3,
                active_days: 5
            }
        );
        // A play today extends the current streak to 4.
        let mut r = rows();
        r.push(row("t9", "A", "X", NOW - 1000.0, 200_000));
        assert_eq!(streaks(&r, &opts, NOW).current_days, 4);
        // Last play two days ago: streak broken.
        let r: Vec<PlayRow> = rows()
            .into_iter()
            .filter(|x| x.played_at < NOW - 1.5 * DAY)
            .collect();
        let s = streaks(&r, &opts, NOW);
        assert_eq!(s.current_days, 0);
        assert_eq!(s.longest_days, 2); // -2,-3 and -5,-6
        assert_eq!(streaks(&[], &opts, NOW), Streaks::default());
        // Time zone changes which day a play lands on.
        let edge = vec![row("e", "A", "X", NOW - 12.5 * 3_600_000.0, 200_000)]; // 23:30Z the previous day
        assert_eq!(streaks(&edge, &opts, NOW).current_days, 1);
        assert_eq!(
            streaks(
                &edge,
                &StatsOptions {
                    tz_offset_minutes: 60,
                    ..Default::default()
                },
                NOW
            )
            .current_days,
            1
        );
        assert_eq!(
            play_date(&edge[0], 60).to_days(),
            local_date(NOW, 60).to_days()
        );
        assert_eq!(
            play_date(&edge[0], 0).to_days(),
            local_date(NOW, 0).to_days() - 1
        );
    }

    #[test]
    fn love_bridge_is_one_way_and_off_by_default() {
        assert_eq!(bridge_love(5, 4, true), Some(true));
        assert_eq!(bridge_love(4, 4, true), Some(true));
        assert_eq!(bridge_love(3, 4, true), None);
        assert_eq!(bridge_love(0, 4, true), None);
        assert_eq!(bridge_love(5, 4, false), None);
        assert_eq!(bridge_love(5, 0, true), None);
        assert_eq!(bridge_love(1, 1, true), Some(true));
    }
}
