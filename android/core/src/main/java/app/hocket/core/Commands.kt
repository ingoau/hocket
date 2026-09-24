package app.hocket.core

import app.hocket.core.api.*

/**
 * Terse constructors for the generated adjacently-tagged commands and queries, so UI code reads as
 * `core.dispatch(Commands.playNext(serverId, ids))` rather than nesting `*Inner` payloads.
 */
object Commands {
    fun seekTo(positionMs: Long) = Command.SeekTo(CommandSeekToInner(positionMs.coerceAtLeast(0).toUInt()))
    fun seekBy(deltaMs: Int) = Command.SeekBy(CommandSeekByInner(deltaMs))
    fun setVolume(volume: Double) = Command.SetVolume(CommandSetVolumeInner(volume))
    fun setNetworkState(state: NetworkState) = Command.SetNetworkState(CommandSetNetworkStateInner(state))
    fun setVisibility(visible: Boolean, focused: Boolean) = Command.SetVisibility(CommandSetVisibilityInner(visible, focused))
    fun setBatterySaver(enabled: Boolean) = Command.SetBatterySaver(CommandSetBatterySaverInner(enabled))
    fun addServer(url: String, username: String, password: String, name: String? = null) =
        Command.AddServer(CommandAddServerInner(url, username, password, name))
    fun removeServer(serverId: ServerId) = Command.RemoveServer(CommandRemoveServerInner(serverId))
    fun probeServer(serverId: ServerId) = Command.ProbeServer(CommandProbeServerInner(serverId))
    fun syncLibrary(serverId: ServerId, full: Boolean) = Command.SyncLibrary(CommandSyncLibraryInner(serverId, full))
    fun setTranscodingProfile(networkId: String?, profile: TranscodingProfile) =
        Command.SetTranscodingProfile(CommandSetTranscodingProfileInner(networkId, profile))

    fun playContext(context: QueueContext, startIndex: Int? = null, shuffle: Boolean = false, saveOutgoing: Boolean = true) =
        Command.PlayContext(CommandPlayContextInner(PlayContextArgs(context, startIndex?.toUInt(), shuffle, saveOutgoing)))
    fun playTracks(serverId: ServerId, trackIds: List<TrackId>, startIndex: Int, label: String, shuffle: Boolean = false) =
        Command.PlayTracks(CommandPlayTracksInner(serverId, trackIds, startIndex.toUInt(), label, shuffle))
    fun playNext(serverId: ServerId, trackIds: List<TrackId>) = Command.PlayNext(CommandPlayNextInner(serverId, trackIds))
    fun playLater(serverId: ServerId, trackIds: List<TrackId>) = Command.PlayLater(CommandPlayLaterInner(serverId, trackIds))
    fun jumpToQueueItem(key: QueueKey) = Command.JumpToQueueItem(CommandJumpToQueueItemInner(key))
    fun removeQueueItems(keys: List<QueueKey>) = Command.RemoveQueueItems(CommandRemoveQueueItemsInner(keys))
    fun moveQueueItem(key: QueueKey, toIndex: Int) = Command.MoveQueueItem(CommandMoveQueueItemInner(key, toIndex.toUInt()))
    fun setShuffle(enabled: Boolean) = Command.SetShuffle(CommandSetShuffleInner(enabled))
    fun setRepeat(mode: RepeatMode) = Command.SetRepeat(CommandSetRepeatInner(mode))
    fun setAutoplay(enabled: Boolean) = Command.SetAutoplay(CommandSetAutoplayInner(enabled))
    fun setQueueMode(mode: QueueMode) = Command.SetQueueMode(CommandSetQueueModeInner(mode))
    fun skipUnavailable(key: QueueKey) = Command.SkipUnavailable(CommandSkipUnavailableInner(key))

    fun restoreSavedQueue(id: String) = Command.RestoreSavedQueue(CommandRestoreSavedQueueInner(id))
    fun pinSavedQueue(id: String, pinned: Boolean) = Command.PinSavedQueue(CommandPinSavedQueueInner(id, pinned))
    fun deleteSavedQueue(id: String) = Command.DeleteSavedQueue(CommandDeleteSavedQueueInner(id))
    fun saveQueueAsPlaylist(savedQueueId: String?, name: String) =
        Command.SaveQueueAsPlaylist(CommandSaveQueueAsPlaylistInner(savedQueueId, name))
    fun setSavedQueueCap(cap: Int) = Command.SetSavedQueueCap(CommandSetSavedQueueCapInner(cap.toUInt()))
    fun undoEntry(id: String) = Command.UndoEntry(CommandUndoEntryInner(id))

    fun setRating(targets: List<RatingTarget>, rating: Int) = Command.SetRating(CommandSetRatingInner(targets, rating.toUInt()))
    fun rateTrack(id: TrackId, rating: Int) = setRating(listOf(RatingTarget.Track(RatingTargetTrackInner(id))), rating)
    fun setLoved(targets: List<RatingTarget>, loved: Boolean) = Command.SetLoved(CommandSetLovedInner(targets, loved))
    fun loveTrack(id: TrackId, loved: Boolean) = setLoved(listOf(RatingTarget.Track(RatingTargetTrackInner(id))), loved)
    fun loveAlbum(id: AlbumId, loved: Boolean) = setLoved(listOf(RatingTarget.Album(RatingTargetAlbumInner(id))), loved)
    fun setArtistLoved(artistId: ArtistId, loved: Boolean) = Command.SetArtistLoved(CommandSetArtistLovedInner(artistId, loved))
    fun createPlaylist(serverId: ServerId, name: String, trackIds: List<TrackId>) =
        Command.CreatePlaylist(CommandCreatePlaylistInner(serverId, name, trackIds))
    fun deletePlaylist(playlistId: PlaylistId) = Command.DeletePlaylist(CommandDeletePlaylistInner(playlistId))
    fun renamePlaylist(playlistId: PlaylistId, name: String, comment: String? = null, public: Boolean? = null) =
        Command.RenamePlaylist(CommandRenamePlaylistInner(playlistId, name, comment, public))
    fun playlistAdd(playlistId: PlaylistId, trackIds: List<TrackId>, atIndex: Int? = null) =
        Command.PlaylistAdd(CommandPlaylistAddInner(playlistId, trackIds, atIndex?.toUInt()))
    fun playlistRemove(playlistId: PlaylistId, indices: List<Int>) =
        Command.PlaylistRemove(CommandPlaylistRemoveInner(playlistId, indices.map { it.toUInt() }))
    fun playlistMove(playlistId: PlaylistId, from: Int, to: Int) =
        Command.PlaylistMove(CommandPlaylistMoveInner(playlistId, from.toUInt(), to.toUInt()))

    fun pin(target: PinTarget, transcode: Boolean = false) = Command.Pin(CommandPinInner(target, transcode))
    fun unpin(target: PinTarget) = Command.Unpin(CommandUnpinInner(target))
    fun setStorageWarnThreshold(bytes: Double?) = Command.SetStorageWarnThreshold(CommandSetStorageWarnThresholdInner(bytes))

    fun cancelJob(id: JobId) = Command.CancelJob(CommandCancelJobInner(id))
    fun retryJob(id: JobId) = Command.RetryJob(CommandRetryJobInner(id))
    fun pauseJob(id: JobId) = Command.PauseJob(CommandPauseJobInner(id))
    fun resumeJob(id: JobId) = Command.ResumeJob(CommandResumeJobInner(id))
    fun retryProblem(id: String) = Command.RetryProblem(CommandRetryProblemInner(id))
    fun dismissProblem(id: String) = Command.DismissProblem(CommandDismissProblemInner(id))

    fun saveFilter(filter: Filter) = Command.SaveFilter(CommandSaveFilterInner(filter))
    fun deleteFilter(id: FilterId) = Command.DeleteFilter(CommandDeleteFilterInner(id))
    fun createSmartPlaylist(serverId: ServerId, filter: Filter, name: String) =
        Command.CreateSmartPlaylist(CommandCreateSmartPlaylistInner(serverId, filter, name))
    fun createStaticPlaylistFromFilter(serverId: ServerId, filter: Filter, name: String) =
        Command.CreateStaticPlaylistFromFilter(CommandCreateStaticPlaylistFromFilterInner(serverId, filter, name))
    fun exportNsp(filter: Filter, path: String? = null) = Command.ExportNsp(CommandExportNspInner(filter, path))
    fun setAutoplaySettings(settings: AutoplaySettings) = Command.SetAutoplaySettings(CommandSetAutoplaySettingsInner(settings))

    fun setLyricsOffset(trackId: TrackId, offsetMs: Int) = Command.SetLyricsOffset(CommandSetLyricsOffsetInner(trackId, offsetMs))
    fun setExternalLyricsEnabled(enabled: Boolean) = Command.SetExternalLyricsEnabled(CommandSetExternalLyricsEnabledInner(enabled))
    fun fetchLyrics(trackId: TrackId) = Command.FetchLyrics(CommandFetchLyricsInner(trackId))

    fun setSetting(key: String, value: String) = Command.SetSetting(CommandSetSettingInner(key, value))
    fun resetSetting(key: String) = Command.ResetSetting(CommandResetSettingInner(key))
    fun setSettingsSync(enabled: Boolean) = Command.SetSettingsSync(CommandSetSettingsSyncInner(enabled))
    fun exportConfig(includeSecrets: Boolean) = Command.ExportConfig(CommandExportConfigInner(includeSecrets))
    fun importConfig(document: String) = Command.ImportConfig(CommandImportConfigInner(document))
    fun setAudioSettings(settings: AudioSettings) = Command.SetAudioSettings(CommandSetAudioSettingsInner(settings))
    fun setOutputDevice(id: String?) = Command.SetOutputDevice(CommandSetOutputDeviceInner(id))

    fun setCoordinatorUrl(url: String?) = Command.SetCoordinatorUrl(CommandSetCoordinatorUrlInner(url))
    fun setLanDiscovery(enabled: Boolean) = Command.SetLanDiscovery(CommandSetLanDiscoveryInner(enabled))
    fun handoffTo(deviceId: DeviceId) = Command.HandoffTo(CommandHandoffToInner(deviceId))
    fun setSleepTimer(timer: SleepTimer?) = Command.SetSleepTimer(CommandSetSleepTimerInner(timer))

    fun backendReport(report: BackendReport) = Command.BackendReport(CommandBackendReportInner(report))
    /** `core_stream`: the backend reads `hocket-stream://` sources through the core. Not persisted: send on every core start. */
    fun setBackendCapabilities(coreStream: Boolean) = Command.SetBackendCapabilities(CommandSetBackendCapabilitiesInner(coreStream))
    fun mediaSessionCommand(action: MediaSessionAction, value: Double? = null) =
        Command.MediaSessionCommand(CommandMediaSessionCommandInner(action, value))
    fun runAction(actionId: String, target: ActionTarget) = Command.RunAction(CommandRunActionInner(actionId, target))
    fun setActionOrder(surface: String, actionIds: List<String>) = Command.SetActionOrder(CommandSetActionOrderInner(surface, actionIds))
    fun setSelection(target: ActionTarget) = Command.SetSelection(CommandSetSelectionInner(target))
    fun touch(target: ActionTarget) = Command.Touch(CommandTouchInner(target))

    // Targets
    fun tracks(ids: List<TrackId>): ActionTarget = ActionTarget.Tracks(ActionTargetTracksInner(ids))
    fun albums(ids: List<AlbumId>): ActionTarget = ActionTarget.Albums(ActionTargetAlbumsInner(ids))
    fun artists(ids: List<ArtistId>): ActionTarget = ActionTarget.Artists(ActionTargetArtistsInner(ids))
    fun playlists(ids: List<PlaylistId>): ActionTarget = ActionTarget.Playlists(ActionTargetPlaylistsInner(ids))
    fun queueItems(keys: List<QueueKey>): ActionTarget = ActionTarget.QueueItems(ActionTargetQueueItemsInner(keys))
    fun savedQueue(id: String): ActionTarget = ActionTarget.SavedQueue(ActionTargetSavedQueueInner(id))

    fun albumContext(serverId: ServerId, id: AlbumId, label: String, sort: SortOrder = SortOrder.Default) =
        QueueContext(serverId, ContextKind.Album(ContextKindAlbumInner(id)), label, sort)
    fun artistContext(serverId: ServerId, id: ArtistId, label: String, sort: SortOrder = SortOrder.Default) =
        QueueContext(serverId, ContextKind.Artist(ContextKindArtistInner(id)), label, sort)
    fun playlistContext(serverId: ServerId, id: PlaylistId, label: String) =
        QueueContext(serverId, ContextKind.Playlist(ContextKindPlaylistInner(id)), label, SortOrder.Default)
    fun genreContext(serverId: ServerId, name: String, sort: SortOrder = SortOrder.Default) =
        QueueContext(serverId, ContextKind.Genre(ContextKindGenreInner(name)), name, sort)
    fun filterContext(serverId: ServerId, filter: Filter) =
        QueueContext(serverId, ContextKind.Filter(ContextKindFilterInner(filter)), filter.name, filter.sort)
    fun adHocContext(serverId: ServerId, label: String, tracks: List<TrackId>, sort: SortOrder = SortOrder.Default) =
        QueueContext(serverId, ContextKind.AdHoc(ContextKindAdHocInner(label)), label, sort, tracks)

    fun trackPin(id: TrackId): PinTarget = PinTarget.Track(PinTargetTrackInner(id))
    fun albumPin(id: AlbumId): PinTarget = PinTarget.Album(PinTargetAlbumInner(id))
    fun playlistPin(id: PlaylistId): PinTarget = PinTarget.Playlist(PinTargetPlaylistInner(id))
}

/** Same for queries. */
object Queries {
    fun tracks(serverId: ServerId, page: Page, sort: SortOrder = SortOrder.Default, descending: Boolean = false, filter: FilterNode? = null) =
        Query.Tracks(QueryTracksInner(serverId, filter, sort, descending, page))
    fun trackCount(serverId: ServerId, filter: FilterNode? = null) = Query.TrackCount(QueryTrackCountInner(serverId, filter))
    fun track(id: TrackId) = Query.Track(QueryTrackInner(id))
    fun tracksByIds(ids: List<TrackId>) = Query.TracksByIds(QueryTracksByIdsInner(ids))
    fun albums(serverId: ServerId, page: Page, sort: SortOrder = SortOrder.Default, descending: Boolean = false, artistId: ArtistId? = null, genre: String? = null) =
        Query.Albums(QueryAlbumsInner(serverId, artistId, genre, sort, descending, page))
    fun albumCount(serverId: ServerId, artistId: ArtistId? = null, genre: String? = null) =
        Query.AlbumCount(QueryAlbumCountInner(serverId, artistId, genre))
    fun album(id: AlbumId) = Query.Album(QueryAlbumInner(id))
    fun albumTracks(id: AlbumId) = Query.AlbumTracks(QueryAlbumTracksInner(id))
    fun artists(serverId: ServerId, page: Page) = Query.Artists(QueryArtistsInner(serverId, page))
    fun artist(id: ArtistId) = Query.Artist(QueryArtistInner(id))
    fun artistTopSongs(id: ArtistId, count: Int) = Query.ArtistTopSongs(QueryArtistTopSongsInner(id, count.toUInt()))
    fun genres(serverId: ServerId) = Query.Genres(QueryGenresInner(serverId))
    fun playlists(serverId: ServerId) = Query.Playlists(QueryPlaylistsInner(serverId))
    fun playlist(id: PlaylistId) = Query.Playlist(QueryPlaylistInner(id))
    fun playlistTracks(id: PlaylistId, page: Page) = Query.PlaylistTracks(QueryPlaylistTracksInner(id, page))
    fun search(serverId: ServerId, query: String, limit: Int, includeServer: Boolean, requestId: String) =
        Query.Search(QuerySearchInner(serverId, query, limit.toUInt(), includeServer, requestId))
    fun lyrics(trackId: TrackId) = Query.Lyrics(QueryLyricsInner(trackId))
    fun related(trackId: TrackId, count: Int) = Query.Related(QueryRelatedInner(trackId, count.toUInt()))
    fun stats(periodDays: Int) = Query.Stats(QueryStatsInner(periodDays.toUInt()))
    fun recentlyPlayed(limit: Int) = Query.RecentlyPlayed(QueryRecentlyPlayedInner(limit.toUInt()))
    fun filterPreview(filter: Filter) = Query.FilterPreview(QueryFilterPreviewInner(filter))
    fun setting(key: String) = Query.Setting(QuerySettingInner(key))
    fun actions(surface: String, target: ActionTarget) = Query.Actions(QueryActionsInner(surface, target))
    fun artwork(id: String, size: Int) = Query.Artwork(QueryArtworkInner(id, size.toUInt()))
    fun mediaSource(trackId: TrackId) = Query.MediaSource(QueryMediaSourceInner(trackId))
    fun configDocument(includeSecrets: Boolean) = Query.ConfigDocument(QueryConfigDocumentInner(includeSecrets))
}

/** The fixed artwork sizes the core caches. Never request anything else. */
object ArtworkSizes {
    const val THUMB = 64
    const val LIST = 160
    const val GRID = 320
    const val FULL = 640
}

/** A [Track] shrunk to what lists and the queue need. */
fun Track.toSummary(): TrackSummary = TrackSummary(
    id = id, serverId = serverId, title = title, artist = artist, album = album, albumId = albumId,
    artistId = artistId, durationMs = durationMs, coverArt = coverArt, rating = rating, loved = loved, offline = offline,
)
