package app.hocket

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "hocket-app")

/**
 * App-only preferences (DataStore). Anything that is not a core registry key lives here: the core
 * refuses unknown settings keys, and these are presentation choices of this app alone.
 */
class AppPrefs(private val context: Context) {
    private val navItemsKey = stringPreferencesKey("navItems")

    /** Ordered navigation item ids, comma-separated; empty = default. */
    val navItems: Flow<List<String>> = context.dataStore.data.map { p -> p[navItemsKey]?.split(',')?.filter { it.isNotBlank() } ?: emptyList() }

    suspend fun setNavItems(ids: List<String>) {
        context.dataStore.edit { it[navItemsKey] = ids.joinToString(",") }
    }
}
