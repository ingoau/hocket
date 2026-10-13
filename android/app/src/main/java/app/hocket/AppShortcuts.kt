package app.hocket

import android.content.Context
import android.content.Intent
import androidx.annotation.DrawableRes
import androidx.annotation.StringRes
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat
import app.hocket.core.QuickPlay
import app.hocket.core.api.Command
import java.security.SecureRandom
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** The launcher shortcuts the user can pick from (long-press on the app icon). */
enum class AppShortcut(val id: String, @StringRes val label: Int, @DrawableRes val icon: Int) {
    Resume("resume", app.hocket.playback.R.string.quickplay_resume, R.drawable.shortcut_resume),
    ShuffleLoved("shuffleLoved", app.hocket.playback.R.string.quickplay_shuffle_loved, R.drawable.shortcut_loved),
    ShuffleAll("shuffleAll", app.hocket.playback.R.string.quickplay_shuffle_all, R.drawable.shortcut_shuffle),
    ShuffleOffline("shuffleOffline", app.hocket.playback.R.string.quickplay_shuffle_offline, R.drawable.shortcut_offline),
    Search("search", R.string.nav_search, R.drawable.shortcut_search);

    /** What the shortcut plays; null for one that only opens a screen. */
    fun command(context: Context, serverId: String): Command? = when (this) {
        Resume -> QuickPlay.resume()
        ShuffleLoved -> QuickPlay.shuffleLoved(serverId, context.getString(label))
        ShuffleAll -> QuickPlay.shuffleAll(serverId, context.getString(label))
        ShuffleOffline -> QuickPlay.shuffleOffline(serverId, context.getString(label))
        Search -> null
    }
}

/**
 * Publishes the chosen [AppShortcut]s as dynamic launcher shortcuts and recognises them when they
 * open [MainActivity].
 *
 * MainActivity is exported (it is the launcher entry), so any app could start it with a shortcut's
 * action; a shortcut starts playback, which other apps may only do with "Allow control by other
 * apps". Each shortcut intent therefore carries a per-install random token (kept in its own
 * preferences file, excluded from backups) that only this app and the launcher holding the shortcut
 * know; an intent without it just opens the app.
 */
object AppShortcuts {
    const val ACTION = "app.hocket.action.SHORTCUT"
    const val EXTRA_ID = "app.hocket.extra.SHORTCUT_ID"
    const val EXTRA_TOKEN = "app.hocket.extra.SHORTCUT_TOKEN"
    private const val PREFS = "hocket-shortcuts"
    private const val KEY_TOKEN = "token"

    val DEFAULT = listOf(AppShortcut.Resume, AppShortcut.ShuffleLoved, AppShortcut.Search)

    /** Launchers show four in the long-press menu (some fewer); more would only be cut. */
    const val MAX = 4

    /** Stored ids to shortcuts: unknown ids dropped, duplicates collapsed, at most [MAX]; null (never chosen) is [DEFAULT]. */
    fun fromIds(ids: List<String>?): List<AppShortcut> =
        ids?.mapNotNull { id -> AppShortcut.entries.firstOrNull { it.id == id } }?.distinct()?.take(MAX) ?: DEFAULT

    fun token(context: Context): String {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        prefs.getString(KEY_TOKEN, null)?.let { return it }
        val bytes = ByteArray(16).also { SecureRandom().nextBytes(it) }
        val token = bytes.joinToString("") { "%02x".format(it) }
        prefs.edit().putString(KEY_TOKEN, token).apply()
        return token
    }

    fun intent(context: Context, shortcut: AppShortcut): Intent = Intent(context, MainActivity::class.java)
        .setAction(ACTION)
        .putExtra(EXTRA_ID, shortcut.id)
        .putExtra(EXTRA_TOKEN, token(context))
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)

    /** The shortcut [intent] stands for, when it is one of ours (token checked); null otherwise. */
    fun parse(context: Context, intent: Intent?): AppShortcut? {
        if (intent?.action != ACTION) return null
        val token = intent.getStringExtra(EXTRA_TOKEN) ?: return null
        if (!java.security.MessageDigest.isEqual(token.toByteArray(), token(context).toByteArray())) return null
        val id = intent.getStringExtra(EXTRA_ID)
        return AppShortcut.entries.firstOrNull { it.id == id }
    }

    /** Replaces the dynamic shortcuts with [shortcuts], in order. Pinned copies keep working. */
    fun publish(context: Context, shortcuts: List<AppShortcut>) {
        val infos = shortcuts.take(ShortcutManagerCompat.getMaxShortcutCountPerActivity(context).coerceAtLeast(1)).mapIndexed { rank, s ->
            ShortcutInfoCompat.Builder(context, s.id)
                .setShortLabel(context.getString(s.label))
                .setIcon(IconCompat.createWithResource(context, s.icon))
                .setIntent(intent(context, s))
                .setRank(rank)
                .build()
        }
        runCatching { ShortcutManagerCompat.setDynamicShortcuts(context, infos) }
    }
}

/**
 * In-app links that arrive from outside the composition (a launcher shortcut, Assistant's search
 * when other apps may not control playback): the shell navigates, the target screen consumes.
 */
object AppLinks {
    /** Open search, with [query] typed in when there is one. A new instance per request. */
    class SearchLink(val query: String?)

    private val _search = MutableStateFlow<SearchLink?>(null)
    val search: StateFlow<SearchLink?> = _search.asStateFlow()

    fun openSearch(query: String?) { _search.value = SearchLink(query?.takeIf { it.isNotBlank() }) }

    fun consume(link: SearchLink) { _search.compareAndSet(link, null) }
}
