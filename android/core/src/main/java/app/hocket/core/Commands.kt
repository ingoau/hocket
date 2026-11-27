package app.hocket.core

import app.hocket.core.api.*

/**
 * Terse constructors for the generated adjacently-tagged commands and queries, so UI code reads as
 * `core.dispatch(Commands.playNext(serverId, ids))` rather than nesting `*Inner` payloads.
 */
object Commands {
    fun setTransitionSettings(settings: TransitionSettings) = Command.SetTransitionSettings(CommandSetTransitionSettingsInner(settings))
    /** The most rows one `Query.QueueRange` returns. */
    const val QUEUE_RANGE_MAX = 201

    fun seekTo(positionMs: Long) = Command.SeekTo(CommandSeekToInner(positionMs.coerceAtLeast(0).toUInt()))
    fun seekBy(deltaMs: Int) = Command.SeekBy(CommandSeekByInner(deltaMs))
    fun setVolume(volume: Double) = Command.SetVolume(CommandSetVolumeInner(volume))
    fun setMuted(muted: Boolean) = Command.SetMuted(CommandSetMutedInner(muted))
    fun previewStart(track: TrackRef) = Command.PreviewStart(CommandPreviewStartInner(track))
    fun previewSeek(positionMs: Long) = Command.PreviewSeek(CommandPreviewSeekInner(positionMs.coerceAtLeast(0).toUInt()))
    fun setNetworkState(state: NetworkState) = Command.SetNetworkState(CommandSetNetworkStateInner(state))
    fun setVisibility(visible: Boolean, focused: Boolean) = Command.SetVisibility(CommandSetVisibilityInner(visible, focused))
    /** The device's own power-save state; battery saver itself is the `battery.level` setting. */
    fun setPowerSave(enabled: Boolean) = Command.SetPowerSave(CommandSetPowerSaveInner(enabled))
    fun addNavidrome(url: String, username: String, password: String, name: String? = null) =
        Command.AddAccount(CommandAddAccountInner(AccountSetup.Navidrome(AccountSetupNavidromeInner(url, username, password, name))))
    /** YouTube Music on this device: signed in with [cookies] (a `Cookie:` header for music.youtube.com), or anonymous. */
    fun addYoutubeMusic(cookies: String?) =
        Command.AddAccount(CommandAddAccountInner(AccountSetup.YoutubeMusic(AccountSetupYoutubeMusicInner(cookies, true))))
    fun setInLibrary(tracks: List<TrackRef>, added: Boolean) = Command.SetInLibrary(CommandSetInLibraryInner(tracks, added))
    fun removeAccount(accountId: AccountId) = Command.RemoveAccount(CommandRemoveAccountInner(accountId))
    fun reorderAccounts(accountIds: List<AccountId>) = Command.ReorderAccounts(CommandReorderAccountsInner(accountIds))
    fun probeAccount(accountId: AccountId) = Command.ProbeAccount(CommandProbeAccountInner(accountId))
    fun syncLibrary(accountId: AccountId, full: Boolean) = Command.SyncLibrary(CommandSyncLibraryInner(accountId, full))
    fun setTranscodingProfile(networkId: String?, profile: TranscodingProfile) =
        Command.SetTranscodingProfile(CommandSetTranscodingProfileInner(networkId, profile))

    fun snooze(tracks: List<TrackRef>, duration: app.hocket.core.api.SnoozeFor) =
        Command.Snooze(CommandSnoozeInner(tracks, duration))

    fun snoozeUntil(tracks: List<TrackRef>, until: Double) =
        Command.SnoozeUntil(CommandSnoozeUntilInner(tracks, until))

    fun unsnooze(tracks: List<TrackRef>) = Command.Unsnooze(CommandUnsnoozeInner(tracks))

    fun playContext(context: QueueContext, startIndex: Int? = null, shuffle: Boolean = false, saveOutgoing: Boolean = true) =
        Command.PlayContext(CommandPlayContextInner(PlayContextArgs(context, startIndex?.toUInt(), shuffle, saveOutgoing)))
    fun resumeAlbum(albumId: AlbumId) = Command.ResumeAlbum(CommandResumeAlbumInner(albumId))
    fun clearAlbumResume(albumId: AlbumId) = Command.ClearAlbumResume(CommandClearAlbumResumeInner(albumId))
    fun playTracks(tracks: List<TrackRef>, startIndex: Int, label: String, shuffle: Boolean = false, startPositionMs: Long = 0) =
        Command.PlayTracks(CommandPlayTracksInner(tracks, startIndex.toUInt(), label, shuffle, startPositionMs.coerceAtLeast(0).toUInt()))
    fun playNext(tracks: List<TrackRef>) = Command.PlayNext(CommandPlayNextInner(tracks))
    fun addToQueue(tracks: List<TrackRef>) = Command.AddToQueue(CommandAddToQueueInner(tracks))
    fun playLater(tracks: List<TrackRef>) = Command.PlayLater(CommandPlayLaterInner(tracks))
    fun jumpToQueueItem(key: QueueKey) = Command.JumpToQueueItem(CommandJumpToQueueItemInner(key))
    fun removeQueueItems(keys: List<QueueKey>) = Command.RemoveQueueItems(CommandRemoveQueueItemsInner(keys))
    /** Moves [key] to just after [after] (the current entry's key for the top of the upcoming list), in or out of the "Next in queue" group. */
    fun moveQueueItem(key: QueueKey, after: QueueKey, inGroup: Boolean) = Command.MoveQueueItem(CommandMoveQueueItemInner(key, after, inGroup))
    /**
     * Songs ([trackIds] new, [keys] queue entries moved or copied as for Play next) directly after
     * the entry [after], in or out of the "Next in queue" group: placed like a drag drop.
     */
    fun insertTracks(tracks: List<TrackRef>, after: QueueKey, inGroup: Boolean, keys: List<QueueKey> = emptyList()) =
        Command.InsertTracks(CommandInsertTracksInner(tracks, keys.ifEmpty { null }, after, inGroup))
    fun queueRange(start: Int, count: Int) = Query.QueueRange(QueryQueueRangeInner(start.coerceAtLeast(0).toUInt(), count.coerceIn(0, QUEUE_RANGE_MAX).toUInt()))
    fun setShuffle(enabled: Boolean) = Command.SetShuffle(CommandSetShuffleInner(enabled))
    fun setRepeat(mode: RepeatMode) = Command.SetRepeat(CommandSetRepeatInner(mode))
    fun setAutoplay(enabled: Boolean) = Command.SetAutoplay(CommandSetAutoplayInner(enabled))
    fun skipUnavailable(key: QueueKey) = Command.SkipUnavailable(CommandSkipUnavailableInner(key))

    fun restoreSavedQueue(id: String) = Command.RestoreSavedQueue(CommandRestoreSavedQueueInner(id))
    fun pinSavedQueue(id: String, pinned: Boolean) = Command.PinSavedQueue(CommandPinSavedQueueInner(id, pinned))
    fun deleteSavedQueue(id: String) = Command.DeleteSavedQueue(CommandDeleteSavedQueueInner(id))
    /** Pins the playing queue (saving it first when it has no saved entry), or unpins its entry. */
    fun pinCurrentQueue(pinned: Boolean) = Command.PinCurrentQueue(CommandPinCurrentQueueInner(pinned))
    /** Removes every unpinned saved queue (undoable). */
    val clearSavedQueues: Command get() = Command.ClearSavedQueues
    /** Renames a saved queue, or the playing one when [savedQueueId] is null; a blank label is ignored. */
    fun renameQueue(savedQueueId: String?, label: String) = Command.RenameQueue(CommandRenameQueueInner(savedQueueId, label))
    fun saveQueueAsPlaylist(savedQueueId: String?, name: String) =
        Command.SaveQueueAsPlaylist(CommandSaveQueueAsPlaylistInner(savedQueueId, name))
    fun setSavedQueueCap(cap: Int) = Command.SetSavedQueueCap(CommandSetSavedQueueCapInner(cap.toUInt()))
    fun undoEntry(id: String) = Command.UndoEntry(CommandUndoEntryInner(id))

    fun setRating(targets: List<RatingTarget>, rating: Int) = Command.SetRating(CommandSetRatingInner(targets, rating.toUInt()))
    fun rateTrack(track: TrackRef, rating: Int) = setRating(listOf(trackTarget(track)), rating)
    /** A song as a rating or love target. */
    fun trackTarget(track: TrackRef): RatingTarget = RatingTarget.Track(RatingTargetTrackInner(track.id, track.source))
    fun setLoved(targets: List<RatingTarget>, loved: Boolean) = Command.SetLoved(CommandSetLovedInner(targets, loved))
    fun loveTrack(track: TrackRef, loved: Boolean) = setLoved(listOf(trackTarget(track)), loved)
    fun loveAlbum(id: AlbumId, loved: Boolean) = setLoved(listOf(RatingTarget.Album(RatingTargetAlbumInner(id))), loved)
    fun setArtistLoved(artistId: ArtistId, loved: Boolean) = Command.SetArtistLoved(CommandSetArtistLovedInner(artistId, loved))
    /** On [accountId], or Mixed with [MIXED_SOURCE]. */
    fun createPlaylist(accountId: AccountId, name: String, tracks: List<TrackRef>) =
        Command.CreatePlaylist(CommandCreatePlaylistInner(accountId, name, tracks))
    fun linkTracks(track: TrackRef, to: TrackRef) = Command.LinkTracks(CommandLinkTracksInner(track, to))
    fun unlinkTrack(track: TrackRef) = Command.UnlinkTrack(CommandUnlinkTrackInner(track))
    fun convertPlaylistToMixed(playlistId: PlaylistId, add: List<TrackRef>) =
        Command.ConvertPlaylistToMixed(CommandConvertPlaylistToMixedInner(playlistId, add))
    fun deletePlaylist(playlistId: PlaylistId) = Command.DeletePlaylist(CommandDeletePlaylistInner(playlistId))
    fun renamePlaylist(playlistId: PlaylistId, name: String, comment: String? = null, public: Boolean? = null) =
        Command.RenamePlaylist(CommandRenamePlaylistInner(playlistId, name, comment, public))
    fun playlistAdd(playlistId: PlaylistId, tracks: List<TrackRef>, atIndex: Int? = null) =
        Command.PlaylistAdd(CommandPlaylistAddInner(playlistId, tracks, atIndex?.toUInt()))
    fun playlistRemove(playlistId: PlaylistId, indices: List<Int>) =
        Command.PlaylistRemove(CommandPlaylistRemoveInner(playlistId, indices.map { it.toUInt() }))
    fun playlistMove(playlistId: PlaylistId, from: Int, to: Int) =
        Command.PlaylistMove(CommandPlaylistMoveInner(playlistId, from.toUInt(), to.toUInt()))

    fun pin(target: PinTarget, transcode: Boolean = false) = Command.Pin(CommandPinInner(target, transcode))
    fun unpin(target: PinTarget) = Command.Unpin(CommandUnpinInner(target))
    /** The album primer (fire and forget): the core routes it to the device that owns playback. */
    fun primeAlbum(albumId: AlbumId) = Command.PrimeAlbum(CommandPrimeAlbumInner(albumId))
    fun primeTrack(track: TrackRef) = Command.PrimeTrack(CommandPrimeTrackInner(track))
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
    fun applySetupSettings(settings: String) = Command.ApplySetupSettings(CommandApplySetupSettingsInner(settings))
    fun setAudioSettings(settings: AudioSettings) = Command.SetAudioSettings(CommandSetAudioSettingsInner(settings))
    fun setOutputDevice(id: String?) = Command.SetOutputDevice(CommandSetOutputDeviceInner(id))

    fun setCoordinatorUrl(url: String?) = Command.SetCoordinatorUrl(CommandSetCoordinatorUrlInner(url))
    fun setLanDiscovery(enabled: Boolean) = Command.SetLanDiscovery(CommandSetLanDiscoveryInner(enabled))
    fun handoffTo(deviceId: DeviceId) = Command.HandoffTo(CommandHandoffToInner(deviceId))
    fun headphonesConnected(name: String?) = Command.HeadphonesConnected(CommandHeadphonesConnectedInner(name))
    fun setSleepTimer(timer: SleepTimer?) = Command.SetSleepTimer(CommandSetSleepTimerInner(timer))

    fun backendReport(report: BackendReport) = Command.BackendReport(CommandBackendReportInner(report))
    /** `core_stream`: the backend reads `hocket-stream://` sources through the core. Not persisted: send on every core start. */
    fun setBackendCapabilities(coreStream: Boolean) = Command.SetBackendCapabilities(CommandSetBackendCapabilitiesInner(coreStream))
    /** Playback goes to the remote renderer (a Cast receiver), or back to this device (null). */
    fun setRenderer(renderer: Renderer?) = Command.SetRenderer(CommandSetRendererInner(renderer))
    /** The renderer's receiver died right after starting [trackId] untranscoded: transcode its format there from now on. */
    fun rendererFailed(trackId: String) = Command.RendererFailed(CommandRendererFailedInner(trackId))
    fun mediaSessionCommand(action: MediaSessionAction, value: Double? = null) =
        Command.MediaSessionCommand(CommandMediaSessionCommandInner(action, value))
    fun runAction(actionId: String, target: ActionTarget) = Command.RunAction(CommandRunActionInner(actionId, target))
    fun setActionOrder(surface: String, actionIds: List<String>) = Command.SetActionOrder(CommandSetActionOrderInner(surface, actionIds))
    fun setSelection(target: ActionTarget) = Command.SetSelection(CommandSetSelectionInner(target))
    fun touch(target: ActionTarget) = Command.Touch(CommandTouchInner(target))

    // Targets
    fun tracks(tracks: List<TrackRef>): ActionTarget = ActionTarget.Tracks(ActionTargetTracksInner(tracks))
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
    fun adHocContext(serverId: ServerId, label: String, tracks: List<TrackRef>, sort: SortOrder = SortOrder.Default) =
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
    fun track(track: TrackRef) = Query.Track(QueryTrackInner(track))
    fun trackSources(track: TrackRef) = Query.TrackSources(QueryTrackSourcesInner(track))
    fun linkCandidates(track: TrackRef, query: String) = Query.LinkCandidates(QueryLinkCandidatesInner(track, query))
    val homeShelves: Query get() = Query.HomeShelves
    /** The server's artist and album info (Last.fm notes, links, similar artists), cached by the core. */
    fun artistInfo(id: ArtistId) = Query.ArtistInfo(QueryArtistInfoInner(id))
    fun albumInfo(id: AlbumId) = Query.AlbumInfo(QueryAlbumInfoInner(id))
    fun tracksByIds(tracks: List<TrackRef>) = Query.TracksByIds(QueryTracksByIdsInner(tracks))
    fun albums(serverId: ServerId, page: Page, sort: SortOrder = SortOrder.Default, descending: Boolean = false, artistId: ArtistId? = null, genre: String? = null) =
        Query.Albums(QueryAlbumsInner(serverId, artistId, genre, sort, descending, page))
    fun albumCount(serverId: ServerId, artistId: ArtistId? = null, genre: String? = null) =
        Query.AlbumCount(QueryAlbumCountInner(serverId, artistId, genre))
    fun album(id: AlbumId) = Query.Album(QueryAlbumInner(id))
    fun albumResume(id: AlbumId) = Query.AlbumResume(QueryAlbumResumeInner(id))
    fun albumTracks(id: AlbumId) = Query.AlbumTracks(QueryAlbumTracksInner(id))
    /** The albums of the tracks matching [filter], each where its first match falls under [sort]. */
    fun albumsOfTracks(serverId: ServerId, limit: Int, sort: SortOrder = SortOrder.Default, descending: Boolean = false, filter: FilterNode? = null) =
        Query.AlbumsOfTracks(QueryAlbumsOfTracksInner(serverId, filter, sort, descending, limit.toUInt()))
    /** Artists by name unless [sort] says album or song count, plays, date added or random. */
    fun artists(serverId: ServerId, page: Page, sort: SortOrder = SortOrder.Default, descending: Boolean = false) =
        Query.Artists(QueryArtistsInner(serverId, page, sort, descending))
    fun artist(id: ArtistId) = Query.Artist(QueryArtistInner(id))
    /** Others' albums the artist is credited on a track of ("Featured on"), newest first. */
    fun artistAppearsOn(id: ArtistId) = Query.ArtistAppearsOn(QueryArtistAppearsOnInner(id))
    fun artistTopSongs(id: ArtistId, count: Int) = Query.ArtistTopSongs(QueryArtistTopSongsInner(id, count.toUInt()))
    fun genres(serverId: ServerId) = Query.Genres(QueryGenresInner(serverId))
    fun playlists(serverId: ServerId) = Query.Playlists(QueryPlaylistsInner(serverId))
    fun playlist(id: PlaylistId) = Query.Playlist(QueryPlaylistInner(id))
    fun playlistTracks(id: PlaylistId, page: Page) = Query.PlaylistTracks(QueryPlaylistTracksInner(id, page))
    /** Every playlist of the server with the indices at which it holds any of [trackIds] (the add to playlist sheet). */
    fun playlistMembership(serverId: ServerId, tracks: List<TrackRef>) = Query.PlaylistMembership(QueryPlaylistMembershipInner(serverId, tracks))
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
    fun createShare(ids: List<String>) = Query.CreateShare(QueryCreateShareInner(ids))
    fun exportAudio(trackId: TrackId) = Query.ExportAudio(QueryExportAudioInner(trackId))
    /** A Squigly URL for a track or an album, made from its Deezer link by the core. */
    fun songLink(trackId: TrackId? = null, albumId: AlbumId? = null) = Query.SongLink(QuerySongLinkInner(trackId, albumId))
}

/** The fixed artwork sizes the core caches. Never request anything else. */
object ArtworkSizes {
    const val THUMB = 64
    const val LIST = 160
    const val GRID = 320
    const val FULL = 640
    /** A detail page's full-bleed header: asked big, shown full-bleed only when it came back at least the header's size (never upscaled). */
    const val HERO = 1200
}

/** A [Track] shrunk to what lists and the queue need. */
fun Track.toSummary(): TrackSummary = TrackSummary(
    id = id, serverId = serverId, title = title, artist = artist, album = album, albumId = albumId,
    artistId = artistId, durationMs = durationMs, coverArt = coverArt, rating = rating, loved = loved, offline = offline,
    artists = artists,
)
