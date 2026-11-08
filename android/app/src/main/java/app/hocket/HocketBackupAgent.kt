package app.hocket

import android.app.backup.BackupAgentHelper
import android.app.backup.BackupManager
import android.app.backup.FileBackupHelper
import android.app.backup.SharedPreferencesBackupHelper
import android.content.Context
import java.io.File

/**
 * Key/value backup of the app-only preferences, in place of Auto Backup.
 *
 * Auto Backup runs the agent in restricted mode and kills the process when it is done
 * (`BackupAgentConnectionManager.unbindAgent(app, allowKill = true)`), even while it is playing:
 * a foreground media service doesn't count as foreground there, so playback stopped mid-track
 * whenever a backup came due. A key/value backup runs in the live process and never kills it
 * (`allowKill = false`).
 *
 * Only what [BackupSet] names travels. Everything else (the library mirror, the device id,
 * credentials, the widget and shortcut state, downloads and caches) never leaves the device.
 */
class HocketBackupAgent : BackupAgentHelper() {
    override fun onCreate() {
        // A restore on a fresh install runs before DataStore has ever made its directory, and
        // FileBackupHelper doesn't create parent directories.
        BackupSet.files.forEach { File(filesDir, it).parentFile?.mkdirs() }
        addHelper("prefs", SharedPreferencesBackupHelper(this, *BackupSet.sharedPrefs.toTypedArray()))
        addHelper("files", FileBackupHelper(this, *BackupSet.files.toTypedArray()))
    }

    companion object {
        /**
         * Key/value backups run only for a package that said its data changed. Called when the app
         * leaves the foreground, which covers anything changed while it was open.
         */
        fun dataChanged(context: Context) {
            runCatching { BackupManager(context).dataChanged() }
        }
    }
}

/** What [HocketBackupAgent] backs up. */
object BackupSet {
    /** SharedPreferences files, by name (no `.xml`). */
    val sharedPrefs = listOf("hocket.shell", "hocket-list-sort", "hocket-recent-searches", "hocket-permissions", "pebble")

    /** Paths under `filesDir`: the app's DataStore preferences (`AppPrefs.kt`). */
    val files = listOf("datastore/$APP_PREFS_DATASTORE.preferences_pb")
}
