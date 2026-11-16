package app.hocket.ui.screens.setup

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Settings → Accounts' Add account: the setup cards, over the app, until done or cancelled. */
object SetupRequests {
    private val _adding = MutableStateFlow(false)
    val adding: StateFlow<Boolean> = _adding.asStateFlow()

    fun addAccount() { _adding.value = true }
    fun done() { _adding.value = false }
}
