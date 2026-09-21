//! `Command` → handler dispatch. One arm per variant; nothing is dropped.

use crate::api::*;
use crate::connect::engine::Input;
use crate::connect::wire::TransportCommand;
use crate::core::actor::Actor;

impl Actor {
    pub(crate) fn handle_command(&mut self, cmd: Command) {
        tracing::trace!(target: "hocket_core", ?cmd, "command");
        match cmd {
            // -- lifecycle --
            Command::Start => self.start(),
            Command::Shutdown => {}
            Command::RequestSnapshot => self.emit_everything(),
            Command::SetNetworkState { state } => self.set_network_state(state),
            Command::SetVisibility { visible, focused } => {
                self.visible = visible;
                self.focused = focused;
            }
            Command::SetBatterySaver { enabled } => self.set_battery_saver(enabled),

            // -- servers --
            Command::AddServer {
                url,
                username,
                password,
                name,
            } => self.add_server(url, username, password, name),
            Command::RemoveServer { server_id } => self.remove_server(server_id),
            Command::ProbeServer { server_id } => self.probe_server(&server_id),
            Command::SyncLibrary { server_id, full } => {
                if self.server_id().as_deref() == Some(server_id.as_str()) {
                    self.maybe_start_sync(full, true);
                }
            }
            Command::SetTranscodingProfile {
                network_id,
                profile,
            } => self.set_transcoding_profile(network_id, profile),

            // -- transport --
            Command::Play => self.transport_command(TransportCommand::Play),
            Command::Pause => self.transport_command(TransportCommand::Pause),
            Command::TogglePlay => self.transport_command(TransportCommand::TogglePlay),
            Command::Stop => self.transport_command(TransportCommand::Stop),
            Command::Next => self.queue_command(Command::Next),
            Command::Previous => self.previous(),
            Command::SeekTo { position_ms } => {
                self.transport_command(TransportCommand::SeekTo { position_ms })
            }
            Command::SeekBy { delta_ms } => {
                self.transport_command(TransportCommand::SeekBy { delta_ms })
            }
            Command::SetVolume { volume } => self.set_volume(volume, true),

            // -- queue --
            Command::PlayContext { args } => self.play_context(args),
            c @ (Command::PlayTracks { .. }
            | Command::PlayNext { .. }
            | Command::PlayLater { .. }
            | Command::JumpToQueueItem { .. }
            | Command::RemoveQueueItems { .. }
            | Command::MoveQueueItem { .. }
            | Command::ClearQueue
            | Command::ClearInsertions
            | Command::SetShuffle { .. }
            | Command::SetRepeat { .. }
            | Command::SetAutoplay { .. }
            | Command::SetQueueMode { .. }
            | Command::SkipUnavailable { .. }
            | Command::PinSavedQueue { .. }
            | Command::DeleteSavedQueue { .. }) => self.queue_command(c),

            // -- saved queues --
            Command::RestoreSavedQueue { id } => self.restore_saved_queue(id),
            Command::SaveQueueAsPlaylist {
                saved_queue_id,
                name,
            } => self.save_queue_as_playlist(saved_queue_id, name),
            Command::SetSavedQueueCap { cap } => {
                self.set_setting_value(
                    crate::settings::keys::QUEUE_SAVED_CAP,
                    serde_json::json!(cap.min(50)),
                );
            }

            // -- undo --
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::UndoEntry { id } => self.undo_to(id),
            Command::RestoreSelection => self.restore_selection(),

            // -- library mutations --
            Command::SetRating { targets, rating } => self.set_rating(targets, rating),
            Command::SetLoved { targets, loved } => self.set_loved(targets, loved),
            Command::SetArtistLoved { artist_id, loved } => self.set_artist_loved(artist_id, loved),
            Command::CreatePlaylist {
                server_id,
                name,
                track_ids,
            } => self.create_playlist(server_id, name, track_ids),
            Command::DeletePlaylist { playlist_id } => self.delete_playlist(playlist_id),
            Command::RenamePlaylist {
                playlist_id,
                name,
                comment,
                public,
            } => self.rename_playlist(playlist_id, name, comment, public),
            Command::PlaylistAdd {
                playlist_id,
                track_ids,
                at_index,
            } => self.playlist_add(playlist_id, track_ids, at_index),
            Command::PlaylistRemove {
                playlist_id,
                indices,
            } => self.playlist_remove(playlist_id, indices),
            Command::PlaylistMove {
                playlist_id,
                from_index,
                to_index,
            } => self.playlist_move(playlist_id, from_index, to_index),
            Command::Scrobble {
                track_id,
                played_at,
                submission,
            } => self.scrobble_command(track_id, played_at, submission),

            // -- downloads --
            Command::Pin { target, transcode } => self.pin(target, transcode),
            Command::Unpin { target } => self.unpin(target),
            Command::ClearStreamCache => self.clear_stream_cache(),
            Command::SetStorageWarnThreshold { bytes } => {
                let v = match bytes {
                    Some(b) => serde_json::json!(b.max(0.0)),
                    None => serde_json::json!(0.0),
                };
                self.set_setting_value(crate::settings::keys::STORAGE_WARN_THRESHOLD_BYTES, v);
            }

            // -- jobs and problems --
            Command::CancelJob { id } => self.job_op("cancel", &id),
            Command::RetryJob { id } => self.job_op("retry", &id),
            Command::PauseJob { id } => self.job_op("pause", &id),
            Command::ResumeJob { id } => self.job_op("resume", &id),
            Command::RetryProblem { id } => self.retry_problem(&id),
            Command::DismissProblem { id } => {
                if let Err(e) = self.jobs.dismiss_problem(&id) {
                    self.error(ErrorKind::Storage, "dismiss problem", Some(e.to_string()));
                }
            }
            Command::DismissAllProblems => {
                if let Err(e) = self.jobs.dismiss_all_problems() {
                    self.error(ErrorKind::Storage, "dismiss problems", Some(e.to_string()));
                }
            }

            // -- filters and autoplay --
            Command::SaveFilter { filter } => self.save_filter(filter),
            Command::DeleteFilter { id } => self.delete_filter(id),
            Command::CreateSmartPlaylist {
                server_id,
                filter,
                name,
            } => self.create_smart_playlist(server_id, filter, name),
            Command::CreateStaticPlaylistFromFilter {
                server_id,
                filter,
                name,
            } => self.create_static_playlist(server_id, filter, name),
            Command::ExportNsp { filter, path } => self.export_nsp(filter, path),
            Command::SetAutoplaySettings { settings } => {
                match serde_json::to_value(&settings) {
                    Ok(v) => self.set_setting_value(crate::settings::keys::AUTOPLAY_SETTINGS, v),
                    Err(e) => self.error(ErrorKind::Internal, "autoplay settings", Some(e.to_string())),
                }
            }

            // -- lyrics --
            Command::SetLyricsOffset {
                track_id,
                offset_ms,
            } => self.set_lyrics_offset(track_id, offset_ms),
            Command::SetExternalLyricsEnabled { enabled } => {
                self.set_setting_value(
                    crate::settings::keys::LYRICS_EXTERNAL_ENABLED,
                    serde_json::json!(enabled),
                );
            }
            Command::FetchLyrics { track_id } => self.fetch_lyrics(track_id, true),

            // -- settings --
            Command::SetSetting { key, value } => self.set_setting_json(&key, &value),
            Command::ResetSetting { key } => self.reset_setting(&key),
            Command::SetSettingsSync { enabled } => {
                self.set_setting_value(
                    crate::settings::keys::SYNC_ENABLED,
                    serde_json::json!(enabled),
                );
            }
            Command::ExportConfig { include_secrets } => {
                let document = self.config_document(include_secrets);
                self.emit(Event::ConfigExported { document });
            }
            Command::ImportConfig { document } => self.import_config(&document),

            // -- audio --
            Command::SetAudioSettings { settings } => self.set_audio_settings(settings),
            Command::SetOutputDevice { id } => {
                let mut a = self.audio.clone();
                a.output_device = id;
                self.set_audio_settings(a);
            }
            Command::RefreshOutputDevices => {
                self.output_devices = self.backend.output_devices();
                self.emit(Event::OutputDevicesChanged {
                    devices: self.output_devices.clone(),
                });
            }

            // -- connect --
            Command::SetCoordinatorUrl { url } => {
                let v = match url {
                    Some(u) if !u.is_empty() => serde_json::json!(u),
                    _ => serde_json::Value::Null,
                };
                self.set_setting_value(crate::settings::keys::CONNECT_COORDINATOR_URL, v);
            }
            Command::ConnectCoordinator => self.engine_input(Input::ConnectCoordinator),
            Command::DisconnectCoordinator => self.engine_input(Input::DisconnectCoordinator),
            Command::SetLanDiscovery { enabled } => {
                self.set_setting_value(
                    crate::settings::keys::CONNECT_LAN_DISCOVERY,
                    serde_json::json!(enabled),
                );
            }
            Command::OpenHandoffPicker => self.engine_input(Input::OpenPicker),
            Command::CloseHandoffPicker => self.engine_input(Input::ClosePicker),
            Command::HandoffTo { device_id } => self.engine_input(Input::HandoffTo { device_id }),
            Command::ResumeHere => {
                self.playback.want_playing = true;
                self.engine_input(Input::ResumeHere);
            }
            Command::DismissResumeOffer => self.engine_input(Input::DismissResume),

            // -- sleep timer --
            Command::SetSleepTimer { timer } => self.set_sleep_timer(timer),

            // -- from the platform --
            Command::BackendReport { report } => match &self.external {
                Some(ext) => ext.report(report),
                None => self.on_backend_report(report),
            },
            Command::MediaSessionCommand { action, value } => self.media_session_command(action, value),
            Command::RunAction { action_id, target } => self.run_action(&action_id, target),
            Command::SetShortcut {
                action_id,
                shortcut,
            } => self.set_shortcut(&action_id, shortcut.as_deref()),
            Command::SetActionOrder {
                surface,
                action_ids,
            } => self.set_action_order(&surface, action_ids),
            Command::SetSelection { target } => self.selection = target,
            Command::Touch { target } => self.touch(target),
        }
    }
}
