package app.hocket

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.map

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "hocket-app")

/** Where the bottom bar's items are kept: ordered ids, empty = the default bar. */
interface NavBarPrefs {
    val navItems: Flow<List<String>>
    suspend fun setNavItems(ids: List<String>)
}

/**
 * App-only preferences (DataStore). Anything that is not a core registry key lives here: the core
 * refuses unknown settings keys, and these are presentation choices of this app alone.
 */
class AppPrefs(private val store: DataStore<Preferences>) : NavBarPrefs {
    constructor(context: Context) : this(context.dataStore)

    private val navItemsKey = stringPreferencesKey("navItems")

    /** Ordered navigation item ids, comma-separated; empty = default. */
    override val navItems: Flow<List<String>> = store.data.map { p -> p[navItemsKey]?.split(',')?.filter { it.isNotBlank() } ?: emptyList() }

    override suspend fun setNavItems(ids: List<String>) {
        store.edit { it[navItemsKey] = ids.joinToString(",") }
    }
}

/** For hosts without the app's DataStore (previews, tests on a bare Application): lives as long as the composition. */
class InMemoryNavBarPrefs(initial: List<String> = emptyList()) : NavBarPrefs {
    private val state = MutableStateFlow(initial)
    override val navItems: Flow<List<String>> = state
    override suspend fun setNavItems(ids: List<String>) { state.value = ids }
}
