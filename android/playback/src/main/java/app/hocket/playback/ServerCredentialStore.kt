package app.hocket.playback

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.security.KeyStore
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** One stored login. The password never appears in logs, toasts or `toString`. */
class ServerCredential(val url: String, val username: String, val password: String, val name: String?) {
    override fun toString(): String = "ServerCredential(url=$url, username=$username)"
}

/** Outcome of looking a stored login up. */
sealed class CredentialLookup {
    data class Found(val credential: ServerCredential) : CredentialLookup()

    /** Nothing is stored for the key. */
    object Missing : CredentialLookup()

    /**
     * A login is stored but cannot be read right now (the keystore threw something other than a
     * tag failure: `keystore2` restarting, OEM quirks after boot or an OS upgrade). It is kept; the
     * user is asked for the password again instead of being silently signed out.
     */
    data class Unavailable(val reason: String) : CredentialLookup()
}

/**
 * Where passwords live on Android. The core persists server metadata only (design: credentials in
 * the platform keystore); the app replays `AddServer` from here on every core start.
 *
 * Passwords are AES/GCM-encrypted with a non-exportable key in the Android Keystore and kept in
 * private SharedPreferences, keyed by `url|username` (the pair the core derives its server id from).
 */
interface ServerCredentialStore {
    /** Every stored `(url, username)`, readable or not. Urls are normalised (no trailing slash). */
    fun keys(): List<Pair<String, String>>
    fun lookup(url: String, username: String): CredentialLookup
    fun get(url: String, username: String): ServerCredential? = (lookup(url, username) as? CredentialLookup.Found)?.credential

    /** Every readable login. Stored-but-unreadable ones are listed by [unavailable] instead. */
    fun all(): List<ServerCredential> = keys().mapNotNull { (url, user) -> get(url, user) }

    /** Keys whose login is stored but cannot be read right now (see [CredentialLookup.Unavailable]). */
    fun unavailable(): List<Pair<String, String>> = keys().filter { (url, user) -> lookup(url, user) is CredentialLookup.Unavailable }
    fun save(credential: ServerCredential)
    fun remove(url: String, username: String)

    /** Drops every credential whose (url, username) is not in [keep]. */
    fun retainOnly(keep: Collection<Pair<String, String>>) {
        val keepSet = keep.map { (url, user) -> url.trimEnd('/') to user }.toSet()
        keys().filter { it !in keepSet }.forEach { (url, user) -> remove(url, user) }
    }

    companion object {
        fun key(url: String, username: String) = url.trimEnd('/') + "|" + username
    }
}

/** In-memory store for tests and previews. */
class InMemoryCredentialStore : ServerCredentialStore {
    private val map = LinkedHashMap<String, ServerCredential>()
    override fun keys() = map.values.map { it.url to it.username }
    override fun lookup(url: String, username: String): CredentialLookup =
        map[ServerCredentialStore.key(url, username)]?.let { CredentialLookup.Found(it) } ?: CredentialLookup.Missing
    override fun save(credential: ServerCredential) {
        val c = ServerCredential(credential.url.trimEnd('/'), credential.username, credential.password, credential.name)
        map[ServerCredentialStore.key(c.url, c.username)] = c
    }
    override fun remove(url: String, username: String) { map.remove(ServerCredentialStore.key(url, username)) }
}

/**
 * The cipher seam of [KeystoreCredentialStore]: AES/GCM with an Android Keystore key in production,
 * a fake in tests. [decrypt] throws [AEADBadTagException] for tampered or foreign ciphertext and any
 * other exception for a transient keystore failure.
 */
interface CredentialCrypto {
    /** False when the key is gone (keystore wiped, data restored on another device): the ciphertext is unrecoverable. */
    fun keyPresent(): Boolean
    /** Returns `iv` to `ciphertext`. */
    fun encrypt(plain: ByteArray): Pair<ByteArray, ByteArray>
    fun decrypt(iv: ByteArray, data: ByteArray): ByteArray
}

class AndroidKeystoreCrypto : CredentialCrypto {
    private companion object {
        const val ALIAS = "hocket-credentials-v1"
        const val GCM_TAG_BITS = 128
    }

    private fun keystore(): KeyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    private fun key(): SecretKey {
        (keystore().getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    // When the keystore itself cannot be opened nothing is known about the key: report it present so
    // the failure is treated as transient rather than as a reason to delete data.
    override fun keyPresent(): Boolean = try { keystore().containsAlias(ALIAS) } catch (e: Exception) { true }

    override fun encrypt(plain: ByteArray): Pair<ByteArray, ByteArray> {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        return cipher.iv to cipher.doFinal(plain)
    }

    override fun decrypt(iv: ByteArray, data: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(GCM_TAG_BITS, iv))
        return cipher.doFinal(data)
    }
}

/**
 * Ciphertext lives in `hocket-credentials` (`url|user` -> `base64(iv):base64(data)`); display names
 * in a separate `hocket-credential-names` file so [keys] never mistakes a name for a login. Both
 * files are excluded from backup and device transfer (the key does not travel).
 *
 * A stored login is deleted only when it can never be read again: the ciphertext is malformed, GCM
 * rejects it ([AEADBadTagException]: tampered, or encrypted with a key that no longer exists), or
 * the key alias is gone. Any other keystore failure is reported as [CredentialLookup.Unavailable]
 * and the entry is kept.
 */
class KeystoreCredentialStore(context: Context, private val crypto: CredentialCrypto = AndroidKeystoreCrypto()) : ServerCredentialStore {
    companion object {
        private const val TAG = "CredentialStore"
        const val PREFS = "hocket-credentials"
        const val NAMES_PREFS = "hocket-credential-names"
        private const val LEGACY_NAME_SUFFIX = "#name"
    }

    private val prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val names = context.applicationContext.getSharedPreferences(NAMES_PREFS, Context.MODE_PRIVATE)

    init {
        // Names used to share the credentials file under `<key>#name`; move them over once.
        prefs.all.keys.filter { it.endsWith(LEGACY_NAME_SUFFIX) }.forEach { legacy ->
            val k = legacy.removeSuffix(LEGACY_NAME_SUFFIX)
            (prefs.all[legacy] as? String)?.let { names.edit().putString(k, it).apply() }
            prefs.edit().remove(legacy).apply()
        }
    }

    override fun keys(): List<Pair<String, String>> = prefs.all.keys
        .filter { !it.contains('#') }
        .mapNotNull { k -> k.split("|", limit = 2).takeIf { it.size == 2 }?.let { it[0] to it[1] } }

    override fun lookup(url: String, username: String): CredentialLookup {
        val k = ServerCredentialStore.key(url, username)
        val stored = prefs.getString(k, null) ?: return CredentialLookup.Missing
        val parts = stored.split(":", limit = 2)
        val (iv, data) = try {
            if (parts.size != 2) throw IllegalArgumentException("no iv")
            Base64.decode(parts[0], Base64.NO_WRAP) to Base64.decode(parts[1], Base64.NO_WRAP)
        } catch (e: IllegalArgumentException) {
            return drop(k, "stored credential is malformed")
        }
        if (!crypto.keyPresent()) return drop(k, "keystore key is gone (wiped or restored from another device)")
        val plain = try {
            crypto.decrypt(iv, data)
        } catch (e: AEADBadTagException) {
            return drop(k, "credential rejected by the keystore key")
        } catch (e: Exception) {
            Log.w(TAG, "credential unreadable right now (${e.javaClass.simpleName}); keeping it, re-login needed")
            return CredentialLookup.Unavailable(e.javaClass.simpleName)
        }
        return CredentialLookup.Found(ServerCredential(url.trimEnd('/'), username, String(plain, Charsets.UTF_8), names.getString(k, null)))
    }

    private fun drop(k: String, why: String): CredentialLookup {
        Log.w(TAG, "$why; dropping it")
        removeKey(k)
        return CredentialLookup.Missing
    }

    override fun save(credential: ServerCredential) {
        val k = ServerCredentialStore.key(credential.url, credential.username)
        try {
            val (iv, data) = crypto.encrypt(credential.password.toByteArray(Charsets.UTF_8))
            prefs.edit().putString(k, Base64.encodeToString(iv, Base64.NO_WRAP) + ":" + Base64.encodeToString(data, Base64.NO_WRAP)).apply()
            names.edit().apply { if (credential.name != null) putString(k, credential.name) else remove(k) }.apply()
        } catch (e: Exception) {
            Log.e(TAG, "could not store credential", e)
        }
    }

    override fun remove(url: String, username: String) = removeKey(ServerCredentialStore.key(url, username))

    private fun removeKey(k: String) {
        prefs.edit().remove(k).remove(k + LEGACY_NAME_SUFFIX).apply()
        names.edit().remove(k).apply()
    }
}
