package app.hocket

import android.content.Context
import app.hocket.core.Commands
import app.hocket.core.HocketJson
import app.hocket.core.SettingKeys
import app.hocket.core.client.CoreClient
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.mapNotNull
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "hocket-app")

/** Where the bottom bar's items are kept: ordered ids, empty = the default bar. */
interface NavBarPrefs {
    val navItems: Flow<List<String>>
    suspend fun setNavItems(ids: List<String>)
}

/**
 * The bar as the synced core setting `nav.mobileBar`, so every phone on the account shares it. It
 * is the phone's own list: the desktop sidebar keeps its order in `actions.order.sidebar`.
 * Nothing is emitted until the core's settings snapshot has the key.
 */
class SyncedNavBarPrefs(private val client: CoreClient) : NavBarPrefs {
    private val ids = ListSerializer(String.serializer())

    override val navItems: Flow<List<String>> = client.settings
        .mapNotNull { it[SettingKeys.NAV_MOBILE_BAR]?.value }
        .map { raw -> runCatching { HocketJson.json.decodeFromString(ids, raw) }.getOrDefault(emptyList()) }
        .distinctUntilChanged()

    override suspend fun setNavItems(ids: List<String>) {
        client.dispatch(Commands.setSetting(SettingKeys.NAV_MOBILE_BAR, HocketJson.json.encodeToString(this.ids, ids)))
    }

    /**
     * One-time move of a bar saved before it synced ([AppPrefs]' device-local copy): taken only
     * when the account has none yet, and the local copy is cleared either way.
     */
    suspend fun adoptLocal(local: AppPrefs) {
        val old = local.navItems.first()
        if (old.isEmpty()) return
        if (navItems.first().isEmpty()) setNavItems(old)
        local.setNavItems(emptyList())
    }
}

/**
 * App-only preferences (DataStore). Anything that is not a core registry key lives here: the core
 * refuses unknown settings keys, and these are presentation choices of this app alone.
 */
class AppPrefs(private val store: DataStore<Preferences>) : NavBarPrefs {
    constructor(context: Context) : this(context.dataStore)

    private val navItemsKey = stringPreferencesKey("navItems")

    /** Ordered navigation item ids, comma-separated; empty = default. Superseded by [SyncedNavBarPrefs], which adopts it once. */
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
