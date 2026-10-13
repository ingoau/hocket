package app.hocket.shortcuts

import android.app.Application
import android.content.Intent
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.AppLinks
import app.hocket.AppShortcut
import app.hocket.AppShortcuts
import app.hocket.MainActivity
import app.hocket.core.QuickPlay
import app.hocket.core.api.Command
import app.hocket.core.api.ContextKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** Launcher shortcuts: the chosen set, what each plays, and that only our own intents count. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class AppShortcutsTest {
    private val app: Application get() = ApplicationProvider.getApplicationContext()

    @Test
    fun storedIdsAreBoundedAndNeverChosenIsTheDefault() {
        assertEquals(AppShortcuts.DEFAULT, AppShortcuts.fromIds(null))
        assertEquals(emptyList<AppShortcut>(), AppShortcuts.fromIds(emptyList()))
        assertEquals(listOf(AppShortcut.Search, AppShortcut.Resume), AppShortcuts.fromIds(listOf("search", "gone", "resume", "search")))
        assertEquals(AppShortcuts.MAX, AppShortcuts.fromIds(AppShortcut.entries.map { it.id }).size)
    }

    @Test
    fun onlyIntentsCarryingTheInstallTokenAreShortcuts() {
        val ours = AppShortcuts.intent(app, AppShortcut.ShuffleLoved)
        assertEquals(AppShortcut.ShuffleLoved, AppShortcuts.parse(app, ours))
        assertEquals("the token is stable", AppShortcuts.token(app), AppShortcuts.token(app))
        val forged = Intent(app, MainActivity::class.java).setAction(AppShortcuts.ACTION).putExtra(AppShortcuts.EXTRA_ID, "shuffleLoved")
        assertNull(AppShortcuts.parse(app, forged))
        assertNull(AppShortcuts.parse(app, Intent(forged).putExtra(AppShortcuts.EXTRA_TOKEN, "guess")))
        assertNull(AppShortcuts.parse(app, Intent(Intent.ACTION_MAIN)))
        assertNull("a relaunch from Recents does not play it again", AppShortcuts.parse(app, Intent(ours).addFlags(Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY)))
    }

    @Test
    fun shufflesAreFilterContextsAndResumeIsPlay() {
        val loved = AppShortcut.ShuffleLoved.command(app, "s1") as Command.PlayContext
        assertTrue(loved.data.args.shuffle)
        assertEquals(QuickPlay.LOVED_FILTER_ID, (loved.data.args.context.kind as ContextKind.Filter).data.filter.id)
        assertEquals(QuickPlay.resume(), AppShortcut.Resume.command(app, "s1"))
        assertNull(AppShortcut.Search.command(app, "s1"))
    }

    @Test
    fun publishingReplacesTheDynamicShortcutsInOrder() {
        AppShortcuts.publish(app, listOf(AppShortcut.Search, AppShortcut.ShuffleAll))
        assertEquals(listOf("search", "shuffleAll"), ShortcutManagerCompat.getDynamicShortcuts(app).sortedBy { it.rank }.map { it.id })
        AppShortcuts.publish(app, emptyList())
        assertTrue(ShortcutManagerCompat.getDynamicShortcuts(app).isEmpty())
    }

    @Test
    fun aSearchLinkIsConsumedOnce() {
        AppLinks.openSearch("  ")
        val link = AppLinks.search.value!!
        assertNull("a blank query is no query", link.query)
        AppLinks.openSearch("coltrane")
        AppLinks.consume(link) // stale: the newer link stays
        assertEquals("coltrane", AppLinks.search.value?.query)
        AppLinks.consume(AppLinks.search.value!!)
        assertSame(null, AppLinks.search.value)
    }
}
