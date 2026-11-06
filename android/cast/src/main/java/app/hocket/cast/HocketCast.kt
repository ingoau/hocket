package app.hocket.cast

import android.content.Context
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.Renderer
import app.hocket.playback.CastController
import app.hocket.playback.CastDevices
import app.hocket.playback.CastListener
import app.hocket.playback.CastProvider
import app.hocket.playback.PlaybackLog
import app.hocket.playback.RendererBackend
import androidx.mediarouter.media.MediaRouteSelector
import androidx.mediarouter.media.MediaRouter
import com.google.android.gms.cast.CastMediaControlIntent
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.OptionsProvider
import com.google.android.gms.cast.framework.SessionManagerListener
import com.google.android.gms.cast.framework.SessionProvider
import com.google.android.gms.cast.framework.media.CastMediaOptions
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import com.google.android.gms.cast.framework.CastOptions as GmsCastOptions

/** The Google Cast SDK [CastProvider]; the app installs it at startup unless it is a FOSS build. */
object HocketCast : CastProvider {
    /** Google's Default Media Receiver: no receiver app of our own to register or host. */
    val RECEIVER_APP_ID: String = CastMediaControlIntent.DEFAULT_MEDIA_RECEIVER_APPLICATION_ID

    fun install() {
        CastDevices.provider = this
    }

    override fun create(context: Context, scope: CoroutineScope, dispatch: (Command) -> Unit, listener: CastListener): CastController? {
        if (GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(context) != ConnectionResult.SUCCESS) {
            PlaybackLog.i("cast: no Google Play services")
            return null
        }
        val cast = runCatching { CastContext.getSharedInstance(context) }
            .onFailure { PlaybackLog.w("cast: ${it.message}") }.getOrNull() ?: return null
        return Controller(context, cast, scope, dispatch, listener)
    }

    override fun routeFeatures(context: Context): List<String> =
        listOf(CastMediaControlIntent.categoryForCast(RECEIVER_APP_ID))

    private class Controller(
        private val context: Context,
        private val cast: CastContext,
        private val scope: CoroutineScope,
        private val dispatch: (Command) -> Unit,
        private val listener: CastListener,
    ) : CastController {
        private val router = MediaRouter.getInstance(context)
        private val selector = MediaRouteSelector.Builder()
            .addControlCategory(CastMediaControlIntent.categoryForCast(RECEIVER_APP_ID))
            .build()
        private var connecting: String? = null
        private var session: CastSession? = null
        private val renderer = CastRenderer(context, scope, dispatch) { error -> stop(error) }
        override val backend: RendererBackend get() = renderer
        /** Set when this end stops the session with something to say about it. */
        private var endError: String? = null
        /** This end is stopping the session. */
        private var stopping = false
        /** Reconnected once since the user picked a receiver, after it crashed on a format. */
        private var reconnected = false

        private val routes = object : MediaRouter.Callback() {
            override fun onRouteAdded(router: MediaRouter, route: MediaRouter.RouteInfo) = publish()
            override fun onRouteRemoved(router: MediaRouter, route: MediaRouter.RouteInfo) = publish()
            override fun onRouteChanged(router: MediaRouter, route: MediaRouter.RouteInfo) = publish()
            override fun onRouteSelected(router: MediaRouter, route: MediaRouter.RouteInfo, reason: Int) = publish()
            override fun onRouteUnselected(router: MediaRouter, route: MediaRouter.RouteInfo, reason: Int) = publish()
        }

        private val sessions = object : SessionManagerListener<CastSession> {
            override fun onSessionStarted(session: CastSession, sessionId: String) = started(session)
            override fun onSessionResumed(session: CastSession, wasSuspended: Boolean) = started(session)
            override fun onSessionStartFailed(session: CastSession, error: Int) {
                PlaybackLog.w("cast: session start failed ($error)")
                val name = connecting?.let(::routeName) ?: context.getString(R.string.cast_receiver)
                connecting = null
                publish()
                listener.onSessionEnded(context.getString(R.string.cast_connect_failed, name), stoppedHere = false)
            }
            override fun onSessionEnded(session: CastSession, error: Int) = ended(session, error)
            // Out of reach (not a crash): nothing to learn from it.
            override fun onSessionResumeFailed(session: CastSession, error: Int) = ended(session, error, mayHaveCrashed = false)
            override fun onSessionStarting(session: CastSession) = Unit
            override fun onSessionEnding(session: CastSession) = Unit
            override fun onSessionResuming(session: CastSession, sessionId: String) = Unit
            // The receiver is out of reach for now; the SDK tries to resume it.
            override fun onSessionSuspended(session: CastSession, reason: Int) = Unit
        }

        init {
            cast.sessionManager.addSessionManagerListener(sessions, CastSession::class.java)
            router.addCallback(selector, routes, 0)
            // A session still up from before this service started (the TV kept playing).
            cast.sessionManager.currentCastSession?.takeIf { it.isConnected }?.let(::started) ?: publish()
        }

        private fun started(session: CastSession) {
            // Already up (the session found at start, then the SDK's own resume of it).
            if (this.session === session) return
            this.session = session
            connecting = null
            renderer.attach(session)
            publish()
            val device = session.castDevice
            val name = device?.friendlyName ?: context.getString(R.string.cast_receiver)
            listener.onSessionStarted(Renderer(device?.deviceId ?: name, name))
        }

        /**
         * The session ended. One the receiver ended with an error right after starting a track
         * untranscoded is taken as it crashing on that track's format (it claims formats it can't
         * play): the core notes the format for this receiver, and this reconnects once, which
         * reloads the track transcoded. Another failure ends casting as usual.
         */
        private fun ended(session: CastSession, error: Int, mayHaveCrashed: Boolean = true) {
            if (this.session !== session) return
            PlaybackLog.i("cast: session ended ($error)")
            val name = session.castDevice?.friendlyName ?: context.getString(R.string.cast_receiver)
            val suspect = if (mayHaveCrashed && error != 0 && !stopping && !reconnected) renderer.crashSuspect() else null
            val route = suspect?.let { routeOf(session) }
            this.session = null
            renderer.detach()
            if (suspect != null && route != null) {
                PlaybackLog.w("cast: $name crashed on ${suspect.track.id} (${suspect.mimeType}); reconnecting transcoded")
                reconnected = true
                endError = null
                dispatch(Commands.rendererFailed(suspect.track.id))
                listener.onSessionEnded(null, stoppedHere = false, reconnecting = true)
                connecting = route
                publish()
                scope.launch {
                    // Let the router and the SDK finish with the old session first.
                    delay(RECONNECT_DELAY_MS)
                    if (connecting != route || this@Controller.session != null) return@launch
                    val r = router.routes.firstOrNull { it.id == route }
                    if (r == null) {
                        connecting = null
                        publish()
                        listener.onSessionEnded(context.getString(R.string.cast_disconnected, name), stoppedHere = false)
                        return@launch
                    }
                    if (r.isSelected) router.unselect(MediaRouter.UNSELECT_REASON_UNKNOWN)
                    router.selectRoute(r)
                }
                return
            }
            reconnected = false
            publish()
            val message = endError ?: if (error != 0) context.getString(R.string.cast_disconnected, name) else null
            endError = null
            val here = stopping
            stopping = false
            listener.onSessionEnded(message, here)
        }

        private fun routeName(id: String): String? = router.routes.firstOrNull { it.id == id }?.name

        /** The route of [session]'s receiver: the selected one, else the one of the same name. */
        private fun routeOf(session: CastSession): String? =
            router.selectedRoute.takeIf { it.matchesSelector(selector) && !it.isDefault }?.id
                ?: router.routes.firstOrNull { it.matchesSelector(selector) && it.name == session.castDevice?.friendlyName }?.id

        private fun publish() {
            val receivers = router.routes.filter { !it.isDefault && !it.isBluetooth && it.isEnabled && it.matchesSelector(selector) }
            // The receiver in use, as a route (the UI matches rows by route id): the selected route,
            // else (a session resumed before the router caught up) the route of the same name.
            val s = session
            val selected = s?.let {
                router.selectedRoute.takeIf { r -> r.matchesSelector(selector) && !r.isDefault }
                    ?: receivers.firstOrNull { r -> r.name == it.castDevice?.friendlyName }
            }
            CastDevices.publish(
                CastDevices.State(
                    available = true,
                    devices = receivers.map { CastDevices.Device(it.id, it.name, it.description) },
                    active = selected?.let { CastDevices.Device(it.id, it.name, it.description) }
                        ?: s?.castDevice?.let { d -> CastDevices.Device(d.deviceId, d.friendlyName ?: "", null) },
                    connecting = connecting,
                ),
            )
        }

        override fun select(id: String) {
            val route = router.routes.firstOrNull { it.id == id } ?: return
            if (route.isSelected && session != null) return
            reconnected = false
            connecting = id
            publish()
            // The Cast SDK starts a session for a selected Cast route ([sessions] hears of it).
            router.selectRoute(route)
        }

        override fun stop() = stop(null)

        private fun stop(error: String?) {
            if (session == null) {
                // Still connecting: give the route up, so the session never starts.
                if (connecting != null) {
                    router.unselect(MediaRouter.UNSELECT_REASON_STOPPED)
                    cast.sessionManager.endCurrentSession(true)
                }
                connecting = null
                publish()
                if (error != null) listener.onSessionEnded(error, stoppedHere = true)
                return
            }
            endError = error
            stopping = true
            cast.sessionManager.endCurrentSession(true)
        }

        override fun scan(active: Boolean) {
            router.addCallback(selector, routes, if (active) MediaRouter.CALLBACK_FLAG_PERFORM_ACTIVE_SCAN or MediaRouter.CALLBACK_FLAG_REQUEST_DISCOVERY else 0)
            publish()
        }

        override fun release() {
            router.removeCallback(routes)
            cast.sessionManager.removeSessionManagerListener(sessions, CastSession::class.java)
            renderer.detach()
            // The session itself stays up: the receiver keeps playing what it has queued, and the
            // next service start resumes it.
        }
    }
}

/** How long a reconnect after a receiver crash waits before selecting the route again. */
private const val RECONNECT_DELAY_MS = 1_000L

/**
 * The Cast SDK's options (named in the manifest): the Default Media Receiver, no media session or
 * notification of the SDK's own (the app's Media3 session is the one), the receiver stopped when
 * the session ends from here, and Cast in the system output switcher, with transfer back to this
 * phone.
 */
class CastOptions : OptionsProvider {
    override fun getCastOptions(context: Context): GmsCastOptions = GmsCastOptions.Builder()
        .setReceiverApplicationId(HocketCast.RECEIVER_APP_ID)
        .setCastMediaOptions(CastMediaOptions.Builder().setMediaSessionEnabled(false).setNotificationOptions(null).build())
        .setStopReceiverApplicationWhenEndingSession(true)
        .setRemoteToLocalEnabled(true)
        .setResumeSavedSession(true)
        .build()

    override fun getAdditionalSessionProviders(context: Context): List<SessionProvider>? = null
}
