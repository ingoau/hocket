package app.hocket.playback

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Process

/**
 * "Allow control by other apps" (`media.externalControl`): whether apps other than the system may
 * connect to the media session, i.e. browse the library, see the queue and control playback
 * (Android Auto, Wear, media browsers, automation apps). Off by default.
 *
 * The system's own controls always work, whatever the setting: this app, and callers holding
 * `MEDIA_CONTENT_CONTROL` (a privileged permission: SystemUI's notification and lock-screen
 * controls, Bluetooth AVRCP, the system itself; checked by uid). Before Android 9 the platform does
 * not say which app sent a legacy `MediaController` command, so those anonymous callers are let
 * through there; otherwise the system's controls would stop working on those versions. Media3's
 * notification controller is checked by the session ([MediaSessionBridge]).
 *
 * The value belongs to the core; this class keeps a copy in SharedPreferences so that a controller
 * connecting while the service starts (before the core's snapshot arrives) is judged by the last
 * known choice rather than by the default. The copy is excluded from backups, like the core's own
 * store.
 */
class ExternalControl(
    context: Context,
    private val sdk: Int = Build.VERSION.SDK_INT,
    /** True when [uid] holds `MEDIA_CONTENT_CONTROL` (or is the system). */
    private val hasMediaContentControl: (uid: Int) -> Boolean = { uid ->
        context.applicationContext.checkPermission(MEDIA_CONTENT_CONTROL, -1, uid) == PackageManager.PERMISSION_GRANTED
    },
) {
    companion object {
        const val PREFS = "hocket-media-control"
        private const val KEY_ALLOWED = "allowed"
        const val MEDIA_CONTENT_CONTROL = "android.permission.MEDIA_CONTENT_CONTROL"
        /** What Media3 reports for a legacy controller the platform did not identify (before API 28). */
        const val LEGACY_CONTROLLER = "android.media.session.MediaController"
    }

    private val prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /** Whether other apps are allowed in now. */
    var allowed: Boolean = prefs.getBoolean(KEY_ALLOWED, false)
        private set

    /** Applies the core's value; true when it changed. */
    fun update(allowed: Boolean): Boolean {
        if (allowed == this.allowed) return false
        this.allowed = allowed
        prefs.edit().putBoolean(KEY_ALLOWED, allowed).apply()
        return true
    }

    /** This app or the system (see the class docs): never subject to the setting. */
    fun isSystem(packageName: String, uid: Int): Boolean = when {
        uid == Process.myUid() -> true
        uid >= 0 && hasMediaContentControl(uid) -> true
        sdk < Build.VERSION_CODES.P && packageName == LEGACY_CONTROLLER -> true
        else -> false
    }

    fun permits(packageName: String, uid: Int): Boolean = allowed || isSystem(packageName, uid)
}
