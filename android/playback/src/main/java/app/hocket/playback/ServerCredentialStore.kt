package app.hocket.playback

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** One stored login. The password never appears in logs, toasts or `toString`. */
class ServerCredential(val url: String, val username: String, val password: String, val name: String?) {
    override fun toString(): String = "ServerCredential(url=$url, username=$username)"
}

/**
 * Where passwords live on Android. The core persists server metadata only (design: credentials in
 * the platform keystore); the app replays `AddServer` from here on every core start.
 *
 * Passwords are AES/GCM-encrypted with a non-exportable key in the Android Keystore and kept in
 * private SharedPreferences, keyed by `url|username` (the pair the core derives its server id from).
 */
interface ServerCredentialStore {
    fun all(): List<ServerCredential>
    fun get(url: String, username: String): ServerCredential?
    fun save(credential: ServerCredential)
    fun remove(url: String, username: String)
    /** Drops every credential whose (url, username) is not in [keep]. */
    fun retainOnly(keep: Collection<Pair<String, String>>) {
        all().filter { (it.url to it.username) !in keep.toSet() }.forEach { remove(it.url, it.username) }
    }

    companion object {
        fun key(url: String, username: String) = url.trimEnd('/') + "|" + username
    }
}

/** In-memory store for tests and previews. */
class InMemoryCredentialStore : ServerCredentialStore {
    private val map = LinkedHashMap<String, ServerCredential>()
    override fun all() = map.values.toList()
    override fun get(url: String, username: String) = map[ServerCredentialStore.key(url, username)]
    override fun save(credential: ServerCredential) { map[ServerCredentialStore.key(credential.url, credential.username)] = credential }
    override fun remove(url: String, username: String) { map.remove(ServerCredentialStore.key(url, username)) }
}

class KeystoreCredentialStore(context: Context) : ServerCredentialStore {
    private companion object {
        const val TAG = "CredentialStore"
        const val PREFS = "hocket-credentials"
        const val ALIAS = "hocket-credentials-v1"
        const val GCM_TAG_BITS = 128
    }

    private val prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
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

    private fun encrypt(plain: String): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val iv = cipher.iv
        val out = cipher.doFinal(plain.toByteArray(Charsets.UTF_8))
        return Base64.encodeToString(iv, Base64.NO_WRAP) + ":" + Base64.encodeToString(out, Base64.NO_WRAP)
    }

    private fun decrypt(stored: String): String? {
        val (ivB64, dataB64) = stored.split(":", limit = 2).takeIf { it.size == 2 } ?: return null
        return try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(GCM_TAG_BITS, Base64.decode(ivB64, Base64.NO_WRAP)))
            String(cipher.doFinal(Base64.decode(dataB64, Base64.NO_WRAP)), Charsets.UTF_8)
        } catch (e: Exception) {
            Log.w(TAG, "credential undecryptable (key rotated or store corrupt); dropping it")
            null
        }
    }

    override fun all(): List<ServerCredential> = prefs.all.keys.mapNotNull { k ->
        val parts = k.split("|", limit = 2)
        if (parts.size != 2) null else get(parts[0], parts[1])
    }

    override fun get(url: String, username: String): ServerCredential? {
        val k = ServerCredentialStore.key(url, username)
        val stored = prefs.getString(k, null) ?: return null
        val password = decrypt(stored) ?: run { prefs.edit().remove(k).apply(); return null }
        return ServerCredential(url.trimEnd('/'), username, password, prefs.getString("$k#name", null))
    }

    override fun save(credential: ServerCredential) {
        val k = ServerCredentialStore.key(credential.url, credential.username)
        try {
            prefs.edit().putString(k, encrypt(credential.password)).apply {
                if (credential.name != null) putString("$k#name", credential.name) else remove("$k#name")
            }.apply()
        } catch (e: Exception) {
            Log.e(TAG, "could not store credential", e)
        }
    }

    override fun remove(url: String, username: String) {
        val k = ServerCredentialStore.key(url, username)
        prefs.edit().remove(k).remove("$k#name").apply()
    }
}
