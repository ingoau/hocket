package app.hocket.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.click
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.blockTouchesBehind
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** A tap on the navigation bar's strip, around its places, never reaches the page behind it. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
class BlockTouchesBehindTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun tapsAroundThePlacesStopAtTheBar() {
        var behind = 0
        var place = 0
        compose.setContent {
            Box(Modifier.size(300.dp)) {
                Box(Modifier.fillMaxSize().clickable { behind++ })
                Box(Modifier.fillMaxSize().blockTouchesBehind().testTag("bar"), contentAlignment = Alignment.Center) {
                    Box(Modifier.size(48.dp).clickable { place++ }.testTag("place"))
                }
            }
        }
        // Beside the place, on the bar's own strip: nothing behind is tapped.
        compose.onNodeWithTag("bar").performTouchInput { click(topLeft + androidx.compose.ui.geometry.Offset(10f, 10f)) }
        compose.waitForIdle()
        assertEquals(0, behind)
        // The place itself still takes its tap.
        compose.onNodeWithTag("place").performClick()
        compose.waitForIdle()
        assertEquals(1, place)
        assertEquals(0, behind)
        // A real tap wobbles a little between down and up: the place still takes it.
        compose.onNodeWithTag("place").performTouchInput { down(center); moveBy(androidx.compose.ui.geometry.Offset(1f, 1f)); up() }
        compose.waitForIdle()
        assertEquals(2, place)
        assertEquals(0, behind)
    }
}
