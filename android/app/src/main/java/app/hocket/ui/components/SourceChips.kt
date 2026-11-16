package app.hocket.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.MIXED_SOURCE
import app.hocket.core.api.AccountInfo
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons

/** One choice of the source filter chip: "" for All. [label] null means the string resource [labelRes]. */
data class SourceOption(val id: String, val label: String?, val labelRes: Int = 0)

object SourceFilter {
    /** The choices: none with one account; else All, each account, then Mixed for playlists. */
    fun options(accounts: List<AccountInfo>, mixed: Boolean): List<SourceOption> {
        if (accounts.size < 2) return emptyList()
        return listOf(SourceOption("", null, R.string.source_all)) +
            accounts.map { SourceOption(it.id, it.name.ifBlank { it.url }) } +
            (if (mixed) listOf(SourceOption(MIXED_SOURCE, null, R.string.source_mixed)) else emptyList())
    }

    /** The account a list query names: the chosen one when it's still offered, else "" (all). */
    fun current(chosen: String, options: List<SourceOption>): String = if (options.any { it.id == chosen }) chosen else ""
}

/** The source the chip has chosen for a list ([mixed]: the playlists, which offer Mixed too). */
@Composable
fun rememberLibrarySource(mixed: Boolean = false): String {
    val client = LocalCoreClient.current
    val accounts by client.accounts.collectAsStateWithLifecycle()
    val chosen by client.librarySource.collectAsStateWithLifecycle()
    return SourceFilter.current(chosen, SourceFilter.options(accounts, mixed))
}

/**
 * The source filter chip (`docs/providers.md`, Phase 3): which account the library, search and
 * playlists list. Only with more than one account; All is the unified library.
 */
@Composable
fun SourceChips(mixed: Boolean = false, modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val accounts by client.accounts.collectAsStateWithLifecycle()
    val chosen by client.librarySource.collectAsStateWithLifecycle()
    val options = SourceFilter.options(accounts, mixed)
    if (options.isEmpty()) return
    val current = SourceFilter.current(chosen, options)
    LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = modifier.padding(bottom = 6.dp).testTag("source.chips")) {
        items(options, key = { it.id.ifEmpty { "all" } }) { o ->
            FilterChip(
                selected = current == o.id,
                onClick = { client.librarySource.value = o.id },
                label = { Text(o.label ?: stringResource(o.labelRes)) },
                leadingIcon = if (current == o.id) ({ Icon(HocketIcons.Filled.Check, null, Modifier.size(FilterChipDefaults.IconSize)) }) else null,
                modifier = Modifier.testTag("source.chip.${if (o.id == MIXED_SOURCE) "mixed" else o.id.ifEmpty { "all" }}"),
            )
        }
    }
}
