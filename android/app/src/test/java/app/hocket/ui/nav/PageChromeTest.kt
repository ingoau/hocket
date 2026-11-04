package app.hocket.ui.nav

import androidx.compose.material3.darkColorScheme
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import org.junit.Assert.assertEquals
import org.junit.Test

class PageChromeTest {
    private val app = darkColorScheme(surface = Color.Black)
    private val album = darkColorScheme(surface = Color(0xFF884466))

    @Test
    fun withNoPageColoursTheBarKeepsTheApps() {
        val chrome = PageChrome()
        assertEquals(app.surface, chrome.chromeScheme("home", app).surface)
    }

    @Test
    fun atRestThePageOnTopColoursTheBar() {
        val chrome = PageChrome()
        chrome.put("album", album, light = false)
        chrome.reportProgress("album", 0f, top = "album", covered = "search")
        assertEquals(album.surface, chrome.chromeScheme("album", app).surface)
    }

    @Test
    fun aPopBlendsFromTheLeavingPageToTheOneUnder() {
        val chrome = PageChrome()
        chrome.put("album", album, light = false)
        // The back stack already names the page under it, from the first frame of the pop.
        chrome.reportProgress("album", 0f, top = "album", covered = "search")
        assertEquals(album.surface, chrome.chromeScheme("search", app).surface)
        chrome.reportProgress("album", 0.5f, top = "album", covered = "search")
        assertEquals(lerp(album.surface, app.surface, 0.5f), chrome.chromeScheme("search", app).surface)
        // Gone: the page now on top.
        chrome.clearProgress("album")
        chrome.remove("album")
        assertEquals(app.surface, chrome.chromeScheme("search", app).surface)
    }

    @Test
    fun aPushBlendsFromThePageUnderToTheOneArriving() {
        val chrome = PageChrome()
        chrome.put("album", album, light = false)
        chrome.reportProgress("album", 1f, top = "album", covered = "search")
        assertEquals(app.surface, chrome.chromeScheme("album", app).surface)
        chrome.reportProgress("album", 0.25f, top = "album", covered = "search")
        assertEquals(lerp(album.surface, app.surface, 0.25f), chrome.chromeScheme("album", app).surface)
    }
}
