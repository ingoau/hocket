//! Reference JSON for the Kotlin side.
//!
//! `cargo test -p hocket-android write_json_fixtures` serialises a representative set of
//! commands, queries, results and events exactly as the core produces them and writes
//! `android/core/src/test/resources/json-fixtures.json`. The Kotlin unit test
//! `JsonRoundTripTest` decodes each with the generated kotlinx types, re-encodes it and
//! compares structurally, so a drift between serde and kotlinx surfaces as a failing JVM test
//! rather than a silent runtime error.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use hocket_core::api::*;
    use serde_json::{json, Value};

    fn summary(id: &str) -> TrackSummary {
        TrackSummary {
            id: id.into(),
            server_id: "srv".into(),
            title: "Title".into(),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            album_id: Some("al1".into()),
            artist_id: None,
            duration_ms: 213_000,
            cover_art: Some("al1".into()),
            rating: 3,
            loved: true,
            offline: OfflineState::Cached,
        }
    }

    fn entry(key: &str, source: QueueSource) -> QueueEntry {
        QueueEntry {
            item: QueueItem {
                key: key.into(),
                track_id: format!("t-{key}"),
                source,
                unavailable: false,
            },
            track: summary(&format!("t-{key}")),
        }
    }

    fn fixtures() -> Vec<(&'static str, Value)> {
        let mut v: Vec<(&str, Value)> = Vec::new();
        let mut push = |name: &'static str, value: Value| v.push((name, value));

        // -- commands ------------------------------------------------------
        push("command.play", json!(Command::Play));
        push("command.start", json!(Command::Start));
        push(
            "command.seekTo",
            json!(Command::SeekTo {
                position_ms: 12_345
            }),
        );
        push(
            "command.seekBy",
            json!(Command::SeekBy { delta_ms: -10_000 }),
        );
        push(
            "command.playNext",
            json!(Command::PlayNext {
                server_id: "srv".into(),
                track_ids: vec!["a".into(), "b".into()]
            }),
        );
        push(
            "command.playTracks",
            json!(Command::PlayTracks {
                server_id: "srv".into(),
                track_ids: vec!["a".into()],
                start_index: 0,
                label: "Search: hello".into(),
                shuffle: false
            }),
        );
        push(
            "command.playContext",
            json!(Command::PlayContext {
                args: PlayContextArgs {
                    context: QueueContext {
                        server_id: "srv".into(),
                        kind: ContextKind::Album { id: "al1".into() },
                        label: "Album".into(),
                        sort: SortOrder::Default,
                        tracks: vec![],
                    },
                    start_index: Some(2),
                    shuffle: true,
                    save_outgoing: true,
                }
            }),
        );
        push(
            "command.setRating",
            json!(Command::SetRating {
                targets: vec![
                    RatingTarget::Track { id: "t1".into() },
                    RatingTarget::Album { id: "al1".into() }
                ],
                rating: 4
            }),
        );
        push(
            "command.setRepeat",
            json!(Command::SetRepeat {
                mode: RepeatMode::One
            }),
        );
        push(
            "command.setNetworkState",
            json!(Command::SetNetworkState {
                state: NetworkState {
                    kind: NetworkKind::Wifi,
                    metered: false,
                    network_id: Some("abc".into())
                }
            }),
        );
        push(
            "command.backendReport.position",
            json!(Command::BackendReport {
                report: BackendReport::Position {
                    key: "k1".into(),
                    position_ms: 5000
                }
            }),
        );
        push(
            "command.backendReport.error",
            json!(Command::BackendReport {
                report: BackendReport::Error {
                    key: "k1".into(),
                    message: "boom".into(),
                    fatal: false
                }
            }),
        );
        push(
            "command.backendReport.focus",
            json!(Command::BackendReport {
                report: BackendReport::AudioFocusLost { transient: true }
            }),
        );
        push(
            "command.mediaSessionCommand",
            json!(Command::MediaSessionCommand {
                action: MediaSessionAction::Seek,
                value: Some(1500.0)
            }),
        );
        push(
            "command.runAction",
            json!(Command::RunAction {
                action_id: "track.love".into(),
                target: ActionTarget::Tracks {
                    ids: vec!["t1".into()]
                }
            }),
        );
        push(
            "command.setSelection.none",
            json!(Command::SetSelection {
                target: ActionTarget::None
            }),
        );
        push(
            "command.pin",
            json!(Command::Pin {
                target: PinTarget::Album { id: "al1".into() },
                transcode: false
            }),
        );
        push(
            "command.setSleepTimer",
            json!(Command::SetSleepTimer {
                timer: Some(SleepTimer {
                    ends_at: Some(1_700_000_000_000.0),
                    stop_at_end_of_track: false
                })
            }),
        );
        push(
            "command.setSleepTimer.none",
            json!(Command::SetSleepTimer { timer: None }),
        );
        push(
            "command.saveFilter",
            json!(Command::SaveFilter {
                filter: Filter {
                    id: "f1".into(),
                    name: "Loved 2020s".into(),
                    root: FilterNode::All(vec![
                        FilterNode::Rule(FilterRule {
                            field: FilterField::Loved,
                            op: FilterOp::IsTrue,
                            value: FilterValue::Bool(true)
                        }),
                        FilterNode::Any(vec![FilterNode::Rule(FilterRule {
                            field: FilterField::Year,
                            op: FilterOp::InTheRange,
                            value: FilterValue::Range {
                                low: 2020.0,
                                high: 2029.0
                            }
                        })]),
                    ]),
                    sort: SortOrder::DateAdded,
                    descending: true,
                    limit: Some(500),
                }
            }),
        );
        push(
            "command.setAudioSettings",
            json!(Command::SetAudioSettings {
                settings: AudioSettings {
                    replay_gain: ReplayGainMode::Album,
                    replay_gain_preamp_db: -3.0,
                    normalisation: true,
                    eq: EqSettings {
                        enabled: true,
                        preamp_db: -2.0,
                        bands: vec![EqBand {
                            frequency_hz: 60.0,
                            gain_db: 3.0,
                            q: 1.0
                        }],
                        preset: Some("Bass".into()),
                    },
                    gapless: true,
                    output_device: None,
                    exclusive: false,
                }
            }),
        );

        // -- queries -------------------------------------------------------
        push("query.snapshot", json!(Query::Snapshot));
        push(
            "query.tracks",
            json!(Query::Tracks {
                server_id: "srv".into(),
                filter: None,
                sort: SortOrder::Title,
                descending: false,
                page: Page {
                    offset: 40,
                    limit: 40
                }
            }),
        );
        push(
            "query.albums",
            json!(Query::Albums {
                server_id: "srv".into(),
                artist_id: Some("ar1".into()),
                genre: None,
                sort: SortOrder::Year,
                descending: true,
                page: Page {
                    offset: 0,
                    limit: 60
                }
            }),
        );
        push(
            "query.artwork",
            json!(Query::Artwork {
                id: "al1".into(),
                size: 160
            }),
        );
        push(
            "query.search",
            json!(Query::Search {
                server_id: "srv".into(),
                query: "hello".into(),
                limit: 20,
                include_server: true,
                request_id: "r1".into()
            }),
        );
        push(
            "query.actions",
            json!(Query::Actions {
                surface: "contextMenu".into(),
                target: ActionTarget::QueueItems {
                    keys: vec!["k1".into()]
                }
            }),
        );

        // -- results -------------------------------------------------------
        push("result.count", json!(QueryResult::Count(42)));
        push("result.path.none", json!(QueryResult::Path(None)));
        push(
            "result.path.some",
            json!(QueryResult::Path(Some("/data/x.jpg".into()))),
        );
        push(
            "result.trackDetail.none",
            json!(QueryResult::TrackDetail(None)),
        );
        push(
            "result.tracks",
            json!(QueryResult::Tracks(TrackPage {
                items: vec![Track {
                    id: "t1".into(),
                    server_id: "srv".into(),
                    title: "Song".into(),
                    duration_ms: 1000,
                    rating: 0,
                    loved: false,
                    play_count: 3,
                    offline: OfflineState::None,
                    explicit: false,
                    ..Default::default()
                }],
                offset: 0,
                total: 1
            })),
        );
        push(
            "result.queue",
            json!(QueryResult::Queue(QueueView {
                context_label: Some("Album".into()),
                history: vec![entry("h1", QueueSource::Context { index: 0 })],
                current: Some(entry("c", QueueSource::Context { index: 1 })),
                playing_next: vec![entry("n1", QueueSource::Inserted)],
                upcoming: vec![entry(
                    "u1",
                    QueueSource::Autoplay {
                        provider: AutoplayProvider::SonicSimilarity,
                        reason: "Similar to X".into(),
                        score: Some(0.91)
                    }
                )],
                shuffle: false,
                repeat: RepeatMode::All,
                autoplay: true,
                mode: QueueMode::Apple,
                total_upcoming: 1,
            })),
        );
        push(
            "result.lyrics",
            json!(QueryResult::LyricsResult(Some(Lyrics {
                track_id: "t1".into(),
                tier: LyricsTier::Syllable,
                lang: Some("en".into()),
                display_artist: None,
                display_title: None,
                agents: vec![
                    LyricsAgent {
                        id: "v1".into(),
                        name: None,
                        side: 0
                    },
                    LyricsAgent {
                        id: "v2".into(),
                        name: Some("B".into()),
                        side: 1
                    }
                ],
                lines: vec![LyricLine {
                    start_ms: Some(1000),
                    end_ms: Some(3000),
                    text: "Hel lo".into(),
                    syllables: vec![
                        LyricSyllable {
                            text: "Hel".into(),
                            start_ms: 1000,
                            end_ms: 1500,
                            joined: true
                        },
                        LyricSyllable {
                            text: "lo".into(),
                            start_ms: 1500,
                            end_ms: 3000,
                            joined: false
                        },
                    ],
                    agent: Some("v1".into()),
                    background: false,
                    translation: None,
                }],
                source: LyricsSource::Server,
                offset_ms: -250,
            }))),
        );

        // -- events --------------------------------------------------------
        push(
            "event.backend.load",
            json!(Event::Backend {
                command: BackendCommand::Load {
                    source: MediaSource {
                        key: "k1".into(),
                        track: summary("t1"),
                        url: "https://nd.example/rest/stream?id=t1".into(),
                        headers: HashMap::from([("X-Test".to_string(), "1".to_string())]),
                        mime_type: Some("audio/flac".into()),
                        gain_db: -6.5,
                        transcoded: false,
                    },
                    next: None,
                    position_ms: 0,
                    play: true,
                }
            }),
        );
        push(
            "event.backend.play",
            json!(Event::Backend {
                command: BackendCommand::Play
            }),
        );
        push(
            "event.mediaSession",
            json!(Event::MediaSession {
                state: MediaSessionState {
                    metadata: Some(MediaSessionMetadata {
                        title: "Song".into(),
                        artist: Some("Artist".into()),
                        album: None,
                        duration_ms: 213_000,
                        artwork_path: Some("/cache/al1-640.jpg".into()),
                        track_id: Some("t1".into()),
                        loved: false,
                        rating: 0,
                    }),
                    is_playing: true,
                    position: PositionStamp {
                        position_ms: 1000,
                        taken_at: 1_700_000_000_000.0,
                        rate: 1.0,
                        is_playing: true
                    },
                    shuffle: false,
                    repeat: RepeatMode::Off,
                    volume: 1.0,
                    actions: vec![
                        MediaSessionAction::Play,
                        MediaSessionAction::Love,
                        MediaSessionAction::Rate
                    ],
                    owns_transport: true,
                }
            }),
        );
        push(
            "event.toast",
            json!(Event::Toast {
                toast: Toast {
                    id: "toast1".into(),
                    message: "Removed 3 tracks".into(),
                    action_label: Some("Undo".into()),
                    action_command: Some(serde_json::to_string(&Command::Undo).unwrap()),
                    duration_ms: 5000,
                }
            }),
        );
        push(
            "event.playerNotice.none",
            json!(Event::PlayerNotice { message: None }),
        );
        push(
            "event.libraryChanged",
            json!(Event::LibraryChanged {
                server_id: "srv".into(),
                tables: vec!["tracks".into()],
                ids: vec![]
            }),
        );
        push(
            "event.nowPlaying.none",
            json!(Event::NowPlayingChanged { entry: None }),
        );
        push(
            "event.handoffPicker",
            json!(Event::HandoffPickerChanged {
                open: true,
                targets: vec![DeviceInfo {
                    id: "d2".into(),
                    name: "Laptop".into(),
                    platform: Platform::Linux,
                    app_version: "0.1.0".into(),
                    playing: false,
                    ready: true,
                    last_seen: 1_700_000_000_000.0,
                    is_self: false,
                }]
            }),
        );
        push(
            "event.error",
            json!(Event::Error {
                kind: ErrorKind::Auth,
                message: "bad password".into(),
                detail: None
            }),
        );
        push(
            "event.sessionChanged",
            json!(Event::SessionChanged {
                document: SessionDocument {
                    schema_version: 1,
                    session_id: "s1".into(),
                    scope: "srv:me".into(),
                    revision: 7,
                    updated_at: 1_700_000_000_000.0,
                    context: None,
                    mode: QueueMode::Apple,
                    cursor: 0,
                    current: None,
                    history: vec![],
                    insertions: vec![],
                    shuffle: None,
                    repeat: RepeatMode::Off,
                    autoplay: false,
                    transport: TransportState::default(),
                    saved_queues: vec![],
                    extra: HashMap::new(),
                }
            }),
        );
        push(
            "config",
            json!(CoreConfig {
                data_dir: "/data".into(),
                cache_dir: "/cache".into(),
                device_id: "dev1".into(),
                device_name: "Pixel".into(),
                platform: Platform::Android,
                app_version: "0.1.0".into(),
                audio: AudioMode::External,
                coordinator_listen: None,
            }),
        );
        v
    }

    /// Writes the fixtures next to the Kotlin tests. Run explicitly; skipped when the
    /// android tree isn't present (e.g. a packaged crate).
    #[test]
    fn write_json_fixtures() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../android/core/src/test/resources");
        if !root.parent().map(|p| p.exists()).unwrap_or(false) {
            eprintln!("android tree not present, skipping fixture write");
            return;
        }
        std::fs::create_dir_all(&root).expect("create fixtures dir");
        let map: serde_json::Map<String, Value> = fixtures()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let text = serde_json::to_string_pretty(&Value::Object(map)).expect("serialise fixtures");
        std::fs::write(root.join("json-fixtures.json"), text + "\n").expect("write fixtures");
    }

    /// The Kotlin side depends on these three wire conventions.
    #[test]
    fn wire_conventions() {
        assert_eq!(json!(Command::Play), json!({"type": "play"}));
        assert_eq!(
            json!(Command::PlayNext {
                server_id: "s".into(),
                track_ids: vec![]
            }),
            json!({"type": "playNext", "data": {"server_id": "s", "track_ids": []}})
        );
        assert_eq!(
            json!(QueryResult::Count(1)),
            json!({"type": "count", "data": 1})
        );
        // Named structs are camelCase; anonymous variant payloads keep snake_case.
        assert_eq!(
            json!(Page {
                offset: 1,
                limit: 2
            }),
            json!({"offset": 1, "limit": 2})
        );
        assert_eq!(
            json!(Query::Artwork {
                id: "x".into(),
                size: 64
            }),
            json!({"type": "artwork", "data": {"id": "x", "size": 64}})
        );
    }
}
