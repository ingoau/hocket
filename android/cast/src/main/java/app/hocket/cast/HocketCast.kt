package app.hocket.cast

import android.content.Context
import app.hocket.core.api.Command
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
        scope: CoroutineScope,
        dispatch: (Command) -> Unit,
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
                listener.onSessionEnded(context.getString(R.string.cast_connect_failed, name))
            }
            override fun onSessionEnded(session: CastSession, error: Int) = ended(session, error)
            override fun onSessionResumeFailed(session: CastSession, error: Int) = ended(session, error)
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
            this.session = session
            connecting = null
            renderer.attach(session)
            publish()
            listener.onSessionStarted(session.castDevice?.friendlyName ?: context.getString(R.string.cast_receiver))
        }

        private fun ended(session: CastSession, error: Int) {
            if (this.session !== session) return
            PlaybackLog.i("cast: session ended ($error)")
            val name = session.castDevice?.friendlyName ?: context.getString(R.string.cast_receiver)
            this.session = null
            renderer.detach()
            publish()
            val message = endError ?: if (error != 0) context.getString(R.string.cast_disconnected, name) else null
            endError = null
            listener.onSessionEnded(message)
        }

        private fun routeName(id: String): String? = router.routes.firstOrNull { it.id == id }?.name

        private fun publish() {
            val receivers = router.routes.filter { !it.isDefault && !it.isBluetooth && it.isEnabled && it.matchesSelector(selector) }
            val selected = router.selectedRoute.takeIf { session != null && it.matchesSelector(selector) }
            CastDevices.publish(
                CastDevices.State(
                    available = true,
                    devices = receivers.map { CastDevices.Device(it.id, it.name, it.description) },
                    active = selected?.let { CastDevices.Device(it.id, it.name, it.description) }
                        ?: session?.castDevice?.let { d -> CastDevices.Device(d.deviceId, d.friendlyName ?: "", null) },
                    connecting = connecting,
                ),
            )
        }

        override fun select(id: String) {
            val route = router.routes.firstOrNull { it.id == id } ?: return
            if (route.isSelected && session != null) return
            connecting = id
            publish()
            // The Cast SDK starts a session for a selected Cast route ([sessions] hears of it).
            router.selectRoute(route)
        }

        override fun stop() = stop(null)

        private fun stop(error: String?) {
            if (session == null) {
                connecting = null
                publish()
                if (error != null) listener.onSessionEnded(error)
                return
            }
            endError = error
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
