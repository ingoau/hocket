package app.hocket.ui.a11y

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.Density
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.TestCore
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.theme.HocketTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w320dp-h690dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class DebugLayoutTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun dump() {
        val core = TestCore()
        compose.setContent {
            val base = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(base.density, fontScale = 2f)) {
                HocketTheme(client = core.client) { CompositionLocalProvider(LocalCoreClient provides core.client) { AppRoot(core.client) } }
            }
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        for (tree in listOf(false, true)) {
            println("=== unmerged=$tree")
            for (n in A11yChecks.all(compose.onRoot(useUnmergedTree = tree).fetchSemanticsNode())) {
                if (n.boundsInRoot.top < 440) continue
                val results = mutableListOf<TextLayoutResult>()
                n.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.invoke(results)
                val r = results.firstOrNull()
                println("${n.id} ${n.config.getOrNull(SemanticsProperties.TestTag)} ${n.config.getOrNull(SemanticsProperties.Text)} ${n.boundsInRoot} size=${n.size} ovH=${r?.didOverflowHeight} lines=${r?.lineCount} h=${r?.size}")
            }
        }
    }
}
