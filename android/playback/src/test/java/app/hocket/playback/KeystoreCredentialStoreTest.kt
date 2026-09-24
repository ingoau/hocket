package app.hocket.playback

import android.app.Application
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.security.KeyStoreException
import javax.crypto.AEADBadTagException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** [KeystoreCredentialStore] over Robolectric SharedPreferences with a fake [CredentialCrypto]. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class KeystoreCredentialStoreTest {
    /** Reversible byte shift; [failWith] and [present] script the keystore's moods. */
    private class FakeCrypto : CredentialCrypto {
        var present = true
        var failWith: Exception? = null
        override fun keyPresent() = present
        override fun encrypt(plain: ByteArray) = byteArrayOf(1, 2, 3) to ByteArray(plain.size) { (plain[it] + 1).toByte() }
        override fun decrypt(iv: ByteArray, data: ByteArray): ByteArray {
            failWith?.let { throw it }
            return ByteArray(data.size) { (data[it] - 1).toByte() }
        }
    }

    private val context: Context get() = ApplicationProvider.getApplicationContext()
    private val crypto = FakeCrypto()
    private lateinit var store: KeystoreCredentialStore
    private val url = "https://music.example.net"
    private val key = ServerCredentialStore.key(url, "alice")
    private fun prefs(name: String) = context.getSharedPreferences(name, Context.MODE_PRIVATE)

    @Before
    fun setUp() {
        prefs(KeystoreCredentialStore.PREFS).edit().clear().commit()
        prefs(KeystoreCredentialStore.NAMES_PREFS).edit().clear().commit()
        store = KeystoreCredentialStore(context, crypto)
    }

    @Test
    fun roundTripWithTheNameInItsOwnFile() {
        store.save(ServerCredential("$url/", "alice", "s3cret", "Home"))
        val c = store.get(url, "alice")!!
        assertEquals("s3cret", c.password)
        assertEquals("Home", c.name)
        assertEquals(url, c.url)
        assertEquals(setOf(key), prefs(KeystoreCredentialStore.PREFS).all.keys)
        assertEquals(setOf(key), prefs(KeystoreCredentialStore.NAMES_PREFS).all.keys)
        assertEquals(listOf(url to "alice"), store.keys())
        assertEquals(1, store.all().size)
        store.remove(url, "alice")
        assertTrue(store.keys().isEmpty())
        assertTrue(prefs(KeystoreCredentialStore.NAMES_PREFS).all.isEmpty())
    }

    @Test
    fun legacyNameEntriesAreMigratedAndNeverTakenForLogins() {
        store.save(ServerCredential(url, "alice", "pw", null))
        prefs(KeystoreCredentialStore.PREFS).edit().putString("$key#name", "Old name").commit()
        val reopened = KeystoreCredentialStore(context, crypto)
        assertEquals("only the real login is a key", listOf(url to "alice"), reopened.keys())
        assertEquals("Old name", reopened.get(url, "alice")?.name)
        assertEquals(1, reopened.all().size)
        assertTrue("the legacy entry moved out of the credentials file", prefs(KeystoreCredentialStore.PREFS).all.keys.none { it.contains('#') })
        assertEquals("pw", reopened.get(url, "alice")?.password)
    }

    @Test
    fun transientKeystoreFailureKeepsTheEntryAndReportsItUnavailable() {
        store.save(ServerCredential(url, "alice", "pw", null))
        crypto.failWith = KeyStoreException("keystore2 restarting")
        assertTrue(store.lookup(url, "alice") is CredentialLookup.Unavailable)
        assertNull(store.get(url, "alice"))
        assertTrue(store.all().isEmpty())
        assertEquals(listOf(url to "alice"), store.unavailable())
        assertTrue("nothing was deleted", prefs(KeystoreCredentialStore.PREFS).contains(key))
        crypto.failWith = null
        assertEquals("readable again once the keystore recovers", "pw", store.get(url, "alice")?.password)
        assertTrue(store.unavailable().isEmpty())
    }

    @Test
    fun rejectedCiphertextIsDeleted() {
        store.save(ServerCredential(url, "alice", "pw", "Home"))
        crypto.failWith = AEADBadTagException("tag mismatch")
        assertEquals(CredentialLookup.Missing, store.lookup(url, "alice"))
        assertTrue(prefs(KeystoreCredentialStore.PREFS).all.isEmpty())
        assertTrue(prefs(KeystoreCredentialStore.NAMES_PREFS).all.isEmpty())
    }

    @Test
    fun missingKeyDeletesTheUnrecoverableEntry() {
        store.save(ServerCredential(url, "alice", "pw", null))
        crypto.present = false
        assertEquals(CredentialLookup.Missing, store.lookup(url, "alice"))
        assertTrue(prefs(KeystoreCredentialStore.PREFS).all.isEmpty())
    }

    @Test
    fun malformedEntryIsDeleted() {
        prefs(KeystoreCredentialStore.PREFS).edit().putString(key, "not-iv-and-data").commit()
        assertEquals(CredentialLookup.Missing, store.lookup(url, "alice"))
        assertTrue(prefs(KeystoreCredentialStore.PREFS).all.isEmpty())
    }

    @Test
    fun retainOnlyDropsEverythingElse() {
        store.save(ServerCredential(url, "alice", "pw", null))
        store.save(ServerCredential("https://other.example.net", "bob", "pw2", null))
        store.retainOnly(listOf("$url/" to "alice"))
        assertEquals(listOf(url to "alice"), store.keys())
        store.retainOnly(emptyList())
        assertTrue(store.keys().isEmpty())
    }
}
