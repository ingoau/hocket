package app.hocket.ui.icons

import androidx.compose.ui.graphics.vector.VectorPath
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * Builds every generated Material Symbol, so a path the parser can't read fails here rather than on
 * the first screen that draws it, and checks each is a mirrored-where-asked 24dp glyph.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
class HocketIconsTest {
    @Test
    fun everyIconBuilds() {
        val icons = HocketIcons.all
        assertEquals("names are unique", icons.size, icons.map { it.name }.toSet().size)
        for (icon in icons) {
            assertEquals(icon.name, 24.dp, icon.defaultWidth)
            assertEquals(icon.name, 24f, icon.viewportWidth)
            assertEquals(icon.name, icon.name.startsWith("AutoMirrored."), icon.autoMirror)
            val paths = icon.root.filterIsInstance<VectorPath>()
            assertTrue("${icon.name} has paths", paths.isNotEmpty())
            assertTrue("${icon.name} has path data", paths.all { it.pathData.isNotEmpty() })
        }
    }

    @Test
    fun filledAndOutlinedDiffer() {
        // The bottom bar marks the selected place by fill alone.
        val filled = HocketIcons.Filled.Home.root[0] as VectorPath
        val outlined = HocketIcons.Outlined.Home.root[0] as VectorPath
        assertTrue(filled.pathData != outlined.pathData)
    }
}
