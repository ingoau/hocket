package app.hocket.ui.components

import androidx.compose.foundation.layout.padding
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.TimePickerDialog
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.hocket.R
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset

/**
 * Snooze's Custom row: a date, then a time, a week from now to start. [onPick] gets the moment
 * (epoch ms, local time) once both are chosen; a time already past is refused by the core.
 */
@Composable
internal fun SnoozeUntilPicker(tag: String, onDismiss: () -> Unit, onPick: (Long) -> Unit) {
    val zone = remember { ZoneId.systemDefault() }
    val start = remember { LocalDateTime.now(zone).plusDays(7) }
    val today = remember { LocalDate.now(zone) }
    var date by remember { mutableStateOf<LocalDate?>(null) }
    val picked = date
    if (picked == null) {
        // The date picker speaks UTC midnights.
        val dateState = rememberDatePickerState(
            initialSelectedDateMillis = start.toLocalDate().atStartOfDay().toInstant(ZoneOffset.UTC).toEpochMilli(),
            selectableDates = remember(today) {
                object : SelectableDates {
                    override fun isSelectableDate(utcTimeMillis: Long) =
                        !Instant.ofEpochMilli(utcTimeMillis).atZone(ZoneOffset.UTC).toLocalDate().isBefore(today)
                    override fun isSelectableYear(year: Int) = year >= today.year
                }
            },
        )
        DatePickerDialog(
            onDismissRequest = onDismiss,
            confirmButton = {
                TextButton(
                    onClick = { dateState.selectedDateMillis?.let { date = Instant.ofEpochMilli(it).atZone(ZoneOffset.UTC).toLocalDate() } },
                    enabled = dateState.selectedDateMillis != null,
                    modifier = Modifier.testTag("$tag.snoozeUntil.next"),
                ) { Text(stringResource(R.string.snooze_custom_next)) }
            },
            dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
            modifier = Modifier.testTag("$tag.snoozeUntil.date"),
        ) {
            DatePicker(dateState, title = { Text(stringResource(R.string.snooze_custom_title), Modifier.padding(start = 24.dp, end = 12.dp, top = 16.dp)) })
        }
    } else {
        val is24 = android.text.format.DateFormat.is24HourFormat(LocalContext.current)
        val timeState = rememberTimePickerState(initialHour = start.hour, initialMinute = start.minute, is24Hour = is24)
        TimePickerDialog(
            onDismissRequest = onDismiss,
            title = { Text(stringResource(R.string.snooze_custom_title)) },
            confirmButton = {
                TextButton(
                    onClick = { onPick(picked.atTime(LocalTime.of(timeState.hour, timeState.minute)).atZone(zone).toInstant().toEpochMilli()) },
                    modifier = Modifier.testTag("$tag.snoozeUntil.ok"),
                ) { Text(stringResource(R.string.action_snooze)) }
            },
            dismissButton = { TextButton(onClick = { date = null }) { Text(stringResource(R.string.action_back)) } },
            modifier = Modifier.testTag("$tag.snoozeUntil.time"),
        ) {
            TimePicker(timeState)
        }
    }
}
