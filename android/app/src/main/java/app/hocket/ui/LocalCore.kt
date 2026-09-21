package app.hocket.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import app.hocket.core.client.CoreClient

/** The client for the whole tree. Screens read state from it and dispatch through it. */
val LocalCoreClient = staticCompositionLocalOf<CoreClient> { error("No CoreClient bound") }

/** Whether the layout is wide (>= 600 dp): two-pane library and a navigation rail. */
val LocalWideLayout = staticCompositionLocalOf { false }

@Composable
fun core(): CoreClient = LocalCoreClient.current
