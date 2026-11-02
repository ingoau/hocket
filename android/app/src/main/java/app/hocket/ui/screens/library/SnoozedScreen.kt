package app.hocket.ui.screens.library

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.toSummary
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.brand.brandTitleStyle
import app.hocket.ui.components.EmptyState
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.rememberSongMenu
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.AccountButton
import app.hocket.ui.nav.BottomContentInset
import java.text.DateFormat
import java.util.Date

/** Everything snoozed, and when each one ends. Tapping a song plays it; Unsnooze is in its menu. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SnoozedScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val snoozes by client.snoozes.collectAsStateWithLifecycle()
    val server by client.server.collectAsStateWithLifecycle()
    val scroll = TopAppBarDefaults.enterAlwaysScrollBehavior()
    val songMenu = rememberSongMenu()
    val whenFormat = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("library.snoozed"),
        topBar = {
            MediumFlexibleTopAppBar(
                title = { Text(stringResource(R.string.snooze_title), style = brandTitleStyle(), maxLines = 1, modifier = Modifier.semantics { heading() }) },
                navigationIcon = { IconButton(onClick = { nav.popBackStack() }) { Icon(HocketIcons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.action_back)) } },
                actions = { AccountButton() },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        if (snoozes.isEmpty()) {
            EmptyState(stringResource(R.string.snooze_title), stringResource(R.string.snooze_empty), Modifier.padding(top = padding.calculateTopPadding()).fillMaxSize())
        } else {
            LazyColumn(
                contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = BottomContentInset),
                modifier = Modifier.fillMaxSize(),
            ) {
                items(snoozes, key = { it.trackId }) { item ->
                    val until = item.until?.let { whenFormat.format(Date(it.toLong())) }
                    val subtitle = if (until == null) stringResource(R.string.snooze_until_never) else stringResource(R.string.snooze_until, until)
                    TrackRow(
                        item.track.toSummary(),
                        onClick = {
                            val serverId = server?.id ?: return@TrackRow
                            client.dispatch(Commands.playTracks(serverId, listOf(item.trackId), 0, item.track.title))
                        },
                        onMore = { songMenu.open(item.track.toSummary()) },
                        subtitle = subtitle,
                        modifier = Modifier.testTag("snoozed.row"),
                    )
                }
            }
        }
    }
}
