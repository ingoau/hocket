package app.hocket.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeDown
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.test.click
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.components.FastScrollbar
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** A long list's thumb shows on scroll, drags the list end to end, and takes no touches hidden. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
class FastScrollbarTest {
    @get:Rule
    val compose = createComposeRule()

    private lateinit var list: LazyListState
    private var tapped = -1

    private fun showList(count: Int) {
        compose.setContent {
            list = rememberLazyListState()
            Box(Modifier.size(320.dp, 400.dp)) {
                LazyColumn(state = list, modifier = Modifier.fillMaxSize().testTag("list")) {
                    items(count) { i -> Box(Modifier.fillMaxWidth().height(60.dp).clickable { tapped = i }) }
                }
                FastScrollbar(list, Modifier.align(Alignment.TopEnd))
            }
        }
    }

    @Test
    fun draggingTheThumbTakesTheListEndToEnd() {
        showList(1000)
        compose.onNodeWithTag("fastScroll.thumb").assertExists()
        // A scroll brings the thumb up.
        compose.onNodeWithTag("list").performTouchInput { swipeUp() }
        compose.waitForIdle()
        // Down to the bottom of the track and past it: the list is at its end.
        compose.onNodeWithTag("fastScroll.thumb").performTouchInput { down(center); moveBy(Offset(0f, 5000f)); up() }
        compose.waitForIdle()
        assertTrue(!list.canScrollForward)
        assertEquals(999, list.layoutInfo.visibleItemsInfo.last().index)
        // Back to the top.
        compose.onNodeWithTag("list").performTouchInput { swipeDown() }
        compose.waitForIdle()
        compose.onNodeWithTag("fastScroll.thumb").performTouchInput { down(center); moveBy(Offset(0f, -5000f)); up() }
        compose.waitForIdle()
        assertEquals(0, list.firstVisibleItemIndex)
        assertEquals(0, list.firstVisibleItemScrollOffset)
        // Half the track down: around the middle of the list.
        compose.onNodeWithTag("list").performTouchInput { swipeUp() }
        compose.waitForIdle()
        val before = list.firstVisibleItemIndex
        compose.onNodeWithTag("fastScroll.thumb").performTouchInput { down(center); moveBy(Offset(0f, (400 - 48) * density / 2f)); up() }
        compose.waitForIdle()
        // Half of the ~993 rows it can scroll through, on from wherever the swipe left it.
        assertTrue("from $before to ${list.firstVisibleItemIndex}", list.firstVisibleItemIndex - before in 465..530)
    }

    @Test
    fun aHiddenThumbLetsTheRowTakeTheTap() {
        showList(1000)
        // Never scrolled, so the thumb is hidden: a tap on it reaches the row under it.
        compose.onNodeWithTag("fastScroll.thumb").performTouchInput { click(center) }
        compose.waitForIdle()
        assertEquals(0, tapped)
        assertEquals(0, list.firstVisibleItemIndex)
    }

    @Test
    fun aShortListHasNoThumb() {
        showList(20)
        compose.onNodeWithTag("list").performTouchInput { swipeUp() }
        compose.waitForIdle()
        compose.onNodeWithTag("fastScroll.thumb").assertDoesNotExist()
    }

    @Test
    fun aGridMovesByRows() {
        lateinit var grid: LazyGridState
        compose.setContent {
            grid = rememberLazyGridState()
            Box(Modifier.size(320.dp, 400.dp)) {
                LazyVerticalGrid(GridCells.Fixed(3), state = grid, modifier = Modifier.fillMaxSize().testTag("grid")) {
                    items(1500) { Box(Modifier.fillMaxWidth().height(120.dp)) }
                }
                FastScrollbar(grid, Modifier.align(Alignment.TopEnd))
            }
        }
        compose.onNodeWithTag("grid").performTouchInput { swipeUp() }
        compose.waitForIdle()
        compose.onNodeWithTag("fastScroll.thumb").performTouchInput { down(center); moveBy(Offset(0f, 5000f)); up() }
        compose.waitForIdle()
        assertTrue(!grid.canScrollForward)
        compose.onNodeWithTag("grid").performTouchInput { swipeDown() }
        compose.waitForIdle()
        compose.onNodeWithTag("fastScroll.thumb").performTouchInput { down(center); moveBy(Offset(0f, -5000f)); up() }
        compose.waitForIdle()
        assertEquals(0, grid.firstVisibleItemIndex)
        compose.onNodeWithTag("grid").performTouchInput { swipeUp() }
        compose.waitForIdle()
        val before = grid.firstVisibleItemIndex
        compose.onNodeWithTag("fastScroll.thumb").performTouchInput { down(center); moveBy(Offset(0f, (400 - 48) * density / 2f)); up() }
        compose.waitForIdle()
        // Half of the ~497 rows it can scroll through, landing on the first item of a row.
        assertTrue("from $before to ${grid.firstVisibleItemIndex}", grid.firstVisibleItemIndex - before in 690..790)
        assertEquals(0, grid.firstVisibleItemIndex % 3)
    }
}
