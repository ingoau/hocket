package app.hocket.playback

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.database.ContentObserver
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.media3.common.HeartRating
import androidx.media3.common.MediaItem
import androidx.media3.common.PercentageRating
import androidx.media3.common.Rating
import androidx.media3.common.StarRating
import androidx.media3.common.ThumbRating
import androidx.media3.common.Player
import androidx.media3.session.CommandButton
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionCommands
import androidx.media3.session.SessionError
import androidx.media3.session.SessionResult
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.QueueView
import app.hocket.core.api.RepeatMode
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/**
 * Builds the Media3 [MediaLibrarySession] over a [CoreSessionPlayer] and keeps its custom button
 * layout in step with the core's customised action list (`MediaSessionState.actions`): love,
 * shuffle, repeat and rate as media button preferences, so they appear on the notification and on
 * Wear/Auto surfaces that honour them. Transport buttons come from the player's available commands.
 *
 * The library side ([browser]) answers other apps' browsing and search (Android Auto, Wear,
 * Assistant, any `MediaBrowser`); a "play …" search request (voice, Auto's search) is resolved to one
 * library item here, before the player dispatches it. Every connected controller may open artwork
 * URIs ([ArtworkProvider.allow]). Browsers asking for the "recent" or "suggested" root get
 * [LibraryBrowser]'s; System UI's recent root is Media3's own.
 *
 * Playback resumption ([onPlaybackResumption]): a play request while nothing is loaded (System UI's
 * resumption card, a headset's play key reaching a stopped service through Media3's
 * `MediaButtonReceiver`) resumes the core's session. That stays an explicit request, as the design
 * requires: the system only asks after the user pressed play. When System UI only asks what it would
 * resume (after a reboot), it gets the session's item without anything playing.
 *
 * Ratings: a controller's standard rating (`setRating`, the legacy session's heart) loves or unloves
 * the track ([HeartRating], [ThumbRating]) or rates it ([StarRating], [PercentageRating], as stars of
 * five), for the current item or for any library or queue id.
 *
 * Who may connect is [control]'s ("Allow control by other apps"): with it off, only the system's own
 * controls, Media3's notification controller and apps with notification access are accepted, and
 * every request is checked again, so turning it off (or revoking an app's notification access) also
 * cuts off controllers that are already connected ([setExternalControl], [listenerObserver]).
 */
class MediaSessionBridge(
    context: Context,
    val player: CoreSessionPlayer,
    private val dispatch: (Command) -> Unit,
    launchIntent: Intent?,
    private val browser: LibraryBrowser,
    private val scope: CoroutineScope,
    private val control: ExternalControl,
) {
    companion object {
        const val CMD_LOVE = "app.hocket.LOVE"
        const val CMD_SHUFFLE = "app.hocket.SHUFFLE"
        const val CMD_REPEAT = "app.hocket.REPEAT"
        const val CMD_RATE = "app.hocket.RATE"
        const val EXTRA_RATING = "rating"

        /**
         * A controller's rating as the core's command: hearts and thumbs love (thumbs down and an
         * unset heart unlove), stars and percentages rate out of five (unset clears the rating).
         * Null for a rating type there is no meaning for.
         */
        fun ratingCommand(trackId: String, rating: Rating): Command? = when (rating) {
            is HeartRating -> Commands.loveTrack(trackId, rating.isRated && rating.isHeart)
            is ThumbRating -> Commands.loveTrack(trackId, rating.isRated && rating.isThumbsUp)
            is StarRating -> Commands.rateTrack(trackId, if (rating.isRated) (rating.starRating / rating.maxStars * 5).roundToInt().coerceIn(0, 5) else 0)
            is PercentageRating -> Commands.rateTrack(trackId, if (rating.isRated) (rating.percent / 20f).roundToInt().coerceIn(0, 5) else 0)
            else -> null
        }
    }

    private val customCommands = listOf(CMD_LOVE, CMD_SHUFFLE, CMD_REPEAT, CMD_RATE).map { SessionCommand(it, Bundle.EMPTY) }
    /** Library commands too: without them a browser's root and children requests are refused. */
    private val sessionCommands = MediaSession.ConnectionResult.DEFAULT_SESSION_AND_LIBRARY_COMMANDS.buildUpon().apply { customCommands.forEach { add(it) } }.build()

    private fun permitted(session: MediaSession, controller: MediaSession.ControllerInfo): Boolean =
        session.isMediaNotificationController(controller) || control.permits(controller.packageName, controller.uid)

    private fun <T : Any> refused(): ListenableFuture<LibraryResult<T>> = Futures.immediateFuture(LibraryResult.ofError<T>(SessionError.ERROR_PERMISSION_DENIED))

    /** Runs a browse request on [scope] and completes the future with its result. */
    private fun <T> async(block: suspend () -> T): ListenableFuture<T> {
        val future = SettableFuture.create<T>()
        scope.launch { runCatching { block() }.fold({ future.set(it) }, { future.setException(it) }) }
        return future
    }

    val session: MediaLibraryService.MediaLibrarySession = MediaLibraryService.MediaLibrarySession.Builder(context, player, object : MediaLibraryService.MediaLibrarySession.Callback {
            override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
                if (!permitted(session, controller)) return MediaSession.ConnectionResult.reject()
                ArtworkProvider.allow(controller.packageName)
                return MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                    .setAvailableSessionCommands(sessionCommands)
                    .setMediaButtonPreferences(buttons(player.state))
                    .build()
            }

            override fun onCustomCommand(session: MediaSession, controller: MediaSession.ControllerInfo, customCommand: SessionCommand, args: Bundle): ListenableFuture<SessionResult> {
                if (!permitted(session, controller)) return Futures.immediateFuture(SessionResult(SessionError.ERROR_PERMISSION_DENIED))
                when (customCommand.customAction) {
                    CMD_LOVE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Love))
                    CMD_SHUFFLE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Shuffle))
                    CMD_REPEAT -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Repeat))
                    CMD_RATE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Rate, args.getInt(EXTRA_RATING, 0).toDouble()))
                    else -> return Futures.immediateFuture(SessionResult(SessionResult.RESULT_ERROR_NOT_SUPPORTED))
                }
                return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
            }

            override fun onPlaybackResumption(session: MediaSession, controller: MediaSession.ControllerInfo, isForPlayback: Boolean): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
                // Not checked against [control]: whoever asks is a controller onConnect already accepted, or
                // Media3's stand-in for a media key that reached the service (from the system, or this
                // app's own guarded receiver and widget), which carries no uid to check.
                // The resume item plays as `MediaSessionAction.Play` ([LibraryBrowser.playNow]): the core
                // carries on with its own session. Nothing loaded: an empty list, and the play that
                // follows is still the core's to judge.
                return async {
                    val item = this@MediaSessionBridge.browser.resumeItem()
                    if (item == null) MediaSession.MediaItemsWithStartPosition(emptyList(), 0, 0L)
                    else MediaSession.MediaItemsWithStartPosition(listOf(item), 0, this@MediaSessionBridge.browser.resumePositionMs())
                }
            }

            override fun onSetRating(session: MediaSession, controller: MediaSession.ControllerInfo, rating: Rating): ListenableFuture<SessionResult> {
                if (!permitted(session, controller)) return Futures.immediateFuture(SessionResult(SessionError.ERROR_PERMISSION_DENIED))
                return Futures.immediateFuture(rate(player.state.metadata?.trackId, rating))
            }

            override fun onSetRating(session: MediaSession, controller: MediaSession.ControllerInfo, mediaId: String, rating: Rating): ListenableFuture<SessionResult> {
                if (!permitted(session, controller)) return Futures.immediateFuture(SessionResult(SessionError.ERROR_PERMISSION_DENIED))
                return async {
                    val trackId = when (val id = MediaId.parse(mediaId)) {
                        is MediaId.QueueItem -> player.trackIdForQueueKey(id.key)
                        null -> if (mediaId == LibraryBrowser.RESUME || mediaId == player.state.metadata?.trackId) player.state.metadata?.trackId else null
                        else -> this@MediaSessionBridge.browser.trackIdFor(mediaId)
                    }
                    rate(trackId, rating)
                }
            }

            override fun onGetLibraryRoot(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<MediaItem>> {
                if (!permitted(session, browser)) return refused()
                val lib = this@MediaSessionBridge.browser
                val root = when {
                    params?.isRecent == true -> lib.recentRoot()
                    params?.isSuggested == true -> lib.suggestedRoot()
                    else -> lib.root()
                }
                return Futures.immediateFuture(LibraryResult.ofItem(root, MediaLibraryService.LibraryParams.Builder()
                    .setRecent(params?.isRecent == true).setSuggested(params?.isSuggested == true).setExtras(LibraryBrowser.rootExtras()).build()))
            }

            override fun onGetChildren(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String, page: Int, pageSize: Int, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
                if (!permitted(session, browser)) return refused()
                return async {
                val children = this@MediaSessionBridge.browser.children(parentId, page, pageSize)
                if (children == null) LibraryResult.ofError(SessionError.ERROR_BAD_VALUE) else LibraryResult.ofItemList(children, params)
                }
            }

            override fun onGetItem(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String): ListenableFuture<LibraryResult<MediaItem>> {
                if (!permitted(session, browser)) return refused()
                return async { this@MediaSessionBridge.browser.item(mediaId)?.let { LibraryResult.ofItem(it, null) } ?: LibraryResult.ofError(SessionError.ERROR_BAD_VALUE) }
            }

            override fun onSearch(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<Void>> {
                if (!permitted(session, browser)) return refused()
                scope.launch {
                    val count = runCatching { this@MediaSessionBridge.browser.search(query).size }.getOrDefault(0)
                    session.notifySearchResultChanged(browser, query, count, params)
                }
                return Futures.immediateFuture(LibraryResult.ofVoid())
            }

            override fun onGetSearchResult(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, page: Int, pageSize: Int, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> {
                if (!permitted(session, browser)) return refused()
                return async {
                val all = this@MediaSessionBridge.browser.search(query)
                val from = (page.toLong() * pageSize).coerceIn(0, all.size.toLong()).toInt()
                LibraryResult.ofItemList(all.subList(from, (from + pageSize.coerceAtLeast(0)).coerceAtMost(all.size)), params)
                }
            }

            override fun onAddMediaItems(mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>): ListenableFuture<MutableList<MediaItem>> {
                if (!permitted(mediaSession, controller)) return Futures.immediateFailedFuture(SecurityException("control by other apps is off"))
                return async {
                // Library ids pass through to the player; a search request ("play X") becomes the item
                // it names. Anything unresolvable is dropped (the player then does nothing).
                mediaItems.mapNotNull { item ->
                    val search = item.requestMetadata.searchQuery
                    when {
                        search != null -> this@MediaSessionBridge.browser.resolveSearch(search, item.requestMetadata.extras)
                        item.mediaId.isNotEmpty() -> item
                        else -> null
                    }
                }.toMutableList()
                }
            }
        })
        .setId("hocket")
        .apply {
            launchIntent?.let {
                setSessionActivity(PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
            }
        }
        .build()

    /** Notification access granted or revoked: connected controllers are judged again. */
    private val listenerObserver = object : ContentObserver(Handler(Looper.getMainLooper())) {
        override fun onChange(selfChange: Boolean) {
            if (control.refreshListeners()) applyAccess()
        }
    }

    private val resolver = context.contentResolver

    init {
        resolver.registerContentObserver(Settings.Secure.getUriFor(ExternalControl.ENABLED_NOTIFICATION_LISTENERS), false, listenerObserver)
    }

    /**
     * The core's "Allow control by other apps" value. Turning it off revokes artwork access and
     * strips every connected controller that is not trusted (the system's, or with notification
     * access) of all commands; turning it on gives those controllers their commands back. Main thread.
     */
    fun setExternalControl(allowed: Boolean) {
        if (control.update(allowed)) applyAccess()
    }

    /** Gives every connected controller the access [control] now permits it. Main thread. */
    private fun applyAccess() {
        if (!control.allowed) ArtworkProvider.revokeAll()
        for (controller in session.connectedControllers) {
            val system = session.isMediaNotificationController(controller) || control.isSystem(controller.packageName, controller.uid)
            when {
                system -> ArtworkProvider.allow(controller.packageName)
                control.permits(controller.packageName, controller.uid) -> {
                    ArtworkProvider.allow(controller.packageName)
                    session.setAvailableCommands(controller, sessionCommands, MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS)
                }
                else -> session.setAvailableCommands(controller, SessionCommands.EMPTY, Player.Commands.EMPTY)
            }
        }
    }

    /** Dispatches [rating] for [trackId] ([ratingCommand]); a bad value when either is unusable. */
    private fun rate(trackId: String?, rating: Rating): SessionResult {
        val command = trackId?.let { ratingCommand(it, rating) } ?: return SessionResult(SessionError.ERROR_BAD_VALUE)
        dispatch(command)
        return SessionResult(SessionResult.RESULT_SUCCESS)
    }

    /** Push a new queue view to the player (its playlist). Main thread. */
    fun applyQueue(queue: QueueView) = player.applyQueue(queue)

    /**
     * The library changed: browsers subscribed to a section refetch it. The count is Media3's
     * "unknown" (`Int.MAX_VALUE`): knowing it would take a query per section.
     */
    fun libraryChanged() {
        for (section in LibrarySection.entries) session.notifyChildrenChanged(section.id, Int.MAX_VALUE, null)
    }

    /** Push a new state to the player and refresh the custom layout. Main thread. */
    fun apply(state: MediaSessionState, clockOffsetMs: Double, remote: Boolean = player.isRemote, remoteVolume: Boolean = false) {
        player.apply(state, clockOffsetMs, remote, remoteVolume)
        session.setMediaButtonPreferences(buttons(state))
    }

    private fun buttons(state: MediaSessionState): List<CommandButton> {
        val meta = state.metadata
        return state.actions.mapNotNull { action ->
            when (action) {
                MediaSessionAction.Love -> CommandButton.Builder(if (meta?.loved == true) CommandButton.ICON_HEART_FILLED else CommandButton.ICON_HEART_UNFILLED)
                    .setDisplayName(if (meta?.loved == true) "Unlove" else "Love")
                    .setSessionCommand(SessionCommand(CMD_LOVE, Bundle.EMPTY)).setEnabled(meta != null).build()
                MediaSessionAction.Shuffle -> CommandButton.Builder(if (state.shuffle) CommandButton.ICON_SHUFFLE_ON else CommandButton.ICON_SHUFFLE_OFF)
                    .setDisplayName("Shuffle").setSessionCommand(SessionCommand(CMD_SHUFFLE, Bundle.EMPTY)).build()
                MediaSessionAction.Repeat -> CommandButton.Builder(when (state.repeat) { RepeatMode.Off -> CommandButton.ICON_REPEAT_OFF; RepeatMode.All -> CommandButton.ICON_REPEAT_ALL; RepeatMode.One -> CommandButton.ICON_REPEAT_ONE })
                    .setDisplayName("Repeat").setSessionCommand(SessionCommand(CMD_REPEAT, Bundle.EMPTY)).build()
                MediaSessionAction.Rate -> CommandButton.Builder(if ((meta?.rating ?: 0u) > 0u) CommandButton.ICON_STAR_FILLED else CommandButton.ICON_STAR_UNFILLED)
                    .setDisplayName("Rate").setSessionCommand(SessionCommand(CMD_RATE, Bundle().apply { putInt(EXTRA_RATING, if ((meta?.rating ?: 0u) >= 5u) 0 else 5) })).setEnabled(meta != null).build()
                else -> null // transport actions are player commands
            }
        }
    }

    fun release() {
        resolver.unregisterContentObserver(listenerObserver)
        session.release()
        player.release()
    }
}
