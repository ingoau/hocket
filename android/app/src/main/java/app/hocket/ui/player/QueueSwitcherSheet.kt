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
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.ui.queue.RecentQueuesList

/**
 * The queue switcher, from "Playing from" in the full player: the recent and pinned queues (the
 * same list as the Recent queues screen); picking one restores it and closes the sheet.
 */
@Composable
fun QueueSwitcherSheet(onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, modifier = Modifier.testTag("queueSwitcher.sheet")) {
        Column(Modifier.navigationBarsPadding()) {
            Text(stringResource(R.string.home_saved_queues), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 24.dp).semantics { heading() })
            RecentQueuesList(Modifier.fillMaxWidth().heightIn(min = 240.dp), contentPadding = PaddingValues(top = 8.dp, bottom = 16.dp), onRestore = onDismiss)
        }
    }
}
