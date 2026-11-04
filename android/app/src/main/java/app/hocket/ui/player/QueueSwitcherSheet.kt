package app.hocket.ui.player

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.ui.queue.RecentQueuesList

/** Opens the Queues drawer (provided by the app shell); null where there is no shell, as in tests. */
val LocalOpenQueues = staticCompositionLocalOf<(() -> Unit)?> { null }

/**
 * Queues, as a drawer wherever it is opened: from the chevron of the full player's queue split
 * button (`player.switchQueue`), the Library's Queues link, Home and a Queues item in the bar. The
 * playing queue above the pinned and recent saved queues; picking one restores it and closes it.
 */
@Composable
fun QueuesSheet(onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, modifier = Modifier.testTag("queueSwitcher.sheet")) {
        Column(Modifier.navigationBarsPadding()) {
            Text(stringResource(R.string.queues_title), style = MaterialTheme.typography.headlineSmall, fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold, modifier = Modifier.padding(start = 24.dp, end = 24.dp, bottom = 8.dp).semantics { heading() })
            RecentQueuesList(Modifier.fillMaxWidth().heightIn(min = 240.dp), contentPadding = PaddingValues(top = 8.dp, bottom = 16.dp), onRestore = onDismiss)
        }
    }
}
