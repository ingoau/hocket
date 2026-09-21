package app.hocket.ui.player

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.api.SleepTimer
import app.hocket.ui.LocalCoreClient

/** Sleep timer: minutes as a connected button group, plus stop-at-end-of-track. */
@Composable
fun SleepTimerSheet(onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val current by client.sleepTimer.collectAsStateWithLifecycle()
    var minutes by remember { mutableIntStateOf(30) }
    var endOfTrack by remember { mutableStateOf(current?.stopAtEndOfTrack ?: false) }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.navigationBarsPadding().padding(horizontal = 24.dp)) {
            Text(stringResource(R.string.sleep_title), style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(16.dp))
            ButtonGroup(overflowIndicator = {}) {
                listOf(15, 30, 45, 60, 90).forEach { m ->
                    toggleableItem(checked = minutes == m && !endOfTrack, label = stringResourceCompat(R.string.sleep_minutes, m), onCheckedChange = { minutes = m; endOfTrack = false })
                }
            }
            Spacer(Modifier.height(16.dp))
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.sleep_end_of_track), modifier = Modifier.weight(1f))
                Switch(checked = endOfTrack, onCheckedChange = { endOfTrack = it })
            }
            Spacer(Modifier.height(16.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = androidx.compose.foundation.layout.Arrangement.End) {
                if (current != null) TextButton(onClick = { client.dispatch(Commands.setSleepTimer(null)); onDismiss() }) { Text(stringResource(R.string.sleep_stop)) }
                Spacer(Modifier.padding(4.dp))
                Button(shapes = ButtonDefaults.shapes(), onClick = {
                    val timer = if (endOfTrack) SleepTimer(null, true) else SleepTimer(System.currentTimeMillis().toDouble() + minutes * 60_000.0, false)
                    client.dispatch(Commands.setSleepTimer(timer)); onDismiss()
                }) { Text(stringResource(R.string.sleep_start)) }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun stringResourceCompat(id: Int, arg: Int): String = stringResource(id, arg)
