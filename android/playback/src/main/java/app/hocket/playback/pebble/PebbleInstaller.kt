package app.hocket.playback.pebble

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.FileProvider
import app.hocket.playback.BuildConfig
import app.hocket.playback.R
import io.rebble.pebblekit2.client.DefaultPebbleAndroidAppPicker
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Serves the bundled watchapp to the Pebble app (a subclass, so it cannot clash with another FileProvider). */
class PebbleFileProvider : FileProvider()

/**
 * The watchapp bundled in the APK (`assets/hocket.pbw`, added by scripts/build-pebble.sh; builds
 * without the Pebble SDK have none) and handing it to Core's Pebble app, which installs a `.pbw`
 * opened with `ACTION_VIEW`, `content://` URIs included. Also remembers the version the watch last
 * reported, for the settings page.
 */
object PebbleInstaller {
    private const val ASSET = "hocket.pbw"
    private const val PREFS = "pebble"
    private const val KEY_WATCH_VERSION = "watchVersion"
    private const val CHANNEL = "pebble"
    private const val NOTIFICATION_ID = 0x9eb

    /** The version this build bundles (pebble/package.json). */
    val bundledVersion: String get() = BuildConfig.PEBBLE_WATCHAPP_VERSION

    fun isBundled(context: Context): Boolean =
        runCatching { context.assets.open(ASSET).close(); true }.getOrDefault(false)

    /** The watchapp version the watch last reported, or null when it has never said hello. */
    fun watchVersion(context: Context): String? =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_WATCH_VERSION, null)

    internal fun recordWatchVersion(context: Context, version: String) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY_WATCH_VERSION, version).apply()
    }

    /** `ACTION_VIEW` of a cached copy of the bundled `.pbw`, aimed at the selected Pebble app when there is one. */
    suspend fun installIntent(context: Context): Intent? {
        if (!withContext(Dispatchers.IO) { isBundled(context) }) return null
        val dir = File(context.cacheDir, "pebble")
        val file = File(dir, ASSET)
        withContext(Dispatchers.IO) { dir.mkdirs(); context.assets.open(ASSET).use { input -> file.outputStream().use { input.copyTo(it) } } }
        val uri = FileProvider.getUriForFile(context, context.packageName + ".pebble", file)
        val pebbleApp = runCatching { DefaultPebbleAndroidAppPicker.getInstance(context).getCurrentlySelectedApp() }.getOrNull()
        return Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/octet-stream")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            .apply { if (pebbleApp != null) setPackage(pebbleApp) }
    }

    /** Opens the Pebble app's installer; false when there is nothing to install or nothing to open it. */
    suspend fun install(context: Context): Boolean {
        val intent = installIntent(context) ?: return false
        return runCatching { context.startActivity(intent) }.isSuccess
    }

    /** "Update the Hocket watchapp", with Install, after the watch reported a different version. */
    suspend fun notifyUpdate(context: Context) {
        val intent = installIntent(context) ?: return
        val manager = NotificationManagerCompat.from(context)
        if (!manager.areNotificationsEnabled()) return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(NotificationChannel(CHANNEL, context.getString(R.string.pebble_channel), NotificationManager.IMPORTANCE_DEFAULT))
        }
        val pending = PendingIntent.getActivity(context, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(androidx.media3.session.R.drawable.media3_notification_small_icon)
            .setContentTitle(context.getString(R.string.pebble_update_title))
            .setContentText(context.getString(R.string.pebble_update_body))
            .setContentIntent(pending)
            .addAction(0, context.getString(R.string.pebble_install), pending)
            .setAutoCancel(true)
            .build()
        runCatching { manager.notify(NOTIFICATION_ID, notification) }
    }
}
