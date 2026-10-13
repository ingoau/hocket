package app.hocket.config

import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

/**
 * The shipped backup rules and manifests, read as XML: what must never leave the device (or arrive
 * on another one) and which components are exported.
 */
class ManifestRulesTest {
    private fun file(path: String): File {
        var dir: File? = File(System.getProperty("user.dir") ?: ".").absoluteFile
        while (dir != null) {
            File(dir, path).takeIf { it.exists() }?.let { return it }
            dir = dir.parentFile
        }
        error("$path not found")
    }

    private fun parse(path: String) = DocumentBuilderFactory.newInstance().apply { isNamespaceAware = true }.newDocumentBuilder().parse(file(path))

    private fun Element.children(tag: String): List<Element> =
        (0 until childNodes.length).map { childNodes.item(it) }.filterIsInstance<Element>().filter { it.tagName == tag }

    /** Everything the device must keep to itself: the library mirror and its backups (large, and a
     * cache), the Connect device id (must be unique per install), the keystore ciphertext and names, and
     * the "allow control by other apps" mirror (device-local), the widget's state (names cache files)
     * and the launcher shortcuts' token (a per-install secret). */
    private val mustExclude = setOf(
        "file:hocket/hocket.sqlite", "file:hocket/hocket.sqlite-wal", "file:hocket/hocket.sqlite-shm", "file:hocket/backups",
        "file:hocket/downloads", "file:hocket/cache",
        "sharedpref:hocket-core.xml", "sharedpref:hocket-credentials.xml", "sharedpref:hocket-credential-names.xml",
        "sharedpref:hocket-media-control.xml", "sharedpref:hocket-widget.xml", "sharedpref:hocket-shortcuts.xml",
    )

    private fun excludes(section: Element): Set<String> = section.children("exclude").map { it.getAttribute("domain") + ":" + it.getAttribute("path") }.toSet()

    @Test
    fun autoBackupNeverCarriesTheMirrorTheDeviceIdOrTheCredentials() {
        val root = parse("android/app/src/main/res/xml/backup_rules.xml").documentElement
        assertEquals("full-backup-content", root.tagName)
        val missing = mustExclude - excludes(root)
        assertTrue("backup_rules.xml must exclude $missing", missing.isEmpty())
    }

    @Test
    fun cloudBackupAndDeviceTransferExcludeTheSame() {
        val root = parse("android/app/src/main/res/xml/data_extraction_rules.xml").documentElement
        for (tag in listOf("cloud-backup", "device-transfer")) {
            val section = root.children(tag).single()
            val missing = mustExclude - excludes(section)
            assertTrue("$tag must exclude $missing", missing.isEmpty())
        }
    }

    private fun elements(path: String, tag: String): List<Element> =
        parse(path).getElementsByTagName(tag).let { list -> (0 until list.length).map { list.item(it) as Element } }

    @Test
    fun theOnlyReceiversAreTheGuardedMediaButtonOneAndTheWidgets() {
        val receivers = listOf("android/playback/src/main/AndroidManifest.xml", "android/app/src/main/AndroidManifest.xml").flatMap { elements(it, "receiver") }
        assertEquals(
            setOf("androidx.media3.session.MediaButtonReceiver", "app.hocket.playback.NowPlayingWidget", "app.hocket.playback.WidgetActionReceiver"),
            receivers.map { it.getAttribute("android:name") }.toSet(),
        )
        // Never the legacy androidx.media receiver: exported unguarded, it let any app start playback.
        // Media3's is exported for the system but only this app (signature) may send to it.
        val mediaButton = receivers.single { it.getAttribute("android:name") == "androidx.media3.session.MediaButtonReceiver" }
        val permission = "\${applicationId}.permission.MEDIA_BUTTON"
        assertEquals(permission, mediaButton.getAttribute("android:permission"))
        val declared = elements("android/playback/src/main/AndroidManifest.xml", "permission").single { it.getAttribute("android:name") == permission }
        assertEquals("signature", declared.getAttribute("android:protectionLevel"))
        // The widget's buttons are reachable only through its own PendingIntents.
        assertEquals("false", receivers.single { it.getAttribute("android:name") == "app.hocket.playback.WidgetActionReceiver" }.getAttribute("android:exported"))
    }

    @Test
    fun theLibraryIsPublishedToMediaBrowsersAndAndroidAuto() {
        val service = parse("android/playback/src/main/AndroidManifest.xml").getElementsByTagName("service").let { list ->
            (0 until list.length).map { list.item(it) as Element }.single { it.getAttribute("android:name") == "app.hocket.playback.PlaybackService" }
        }
        val actions = service.getElementsByTagName("action").let { list -> (0 until list.length).map { (list.item(it) as Element).getAttribute("android:name") } }
        assertTrue(actions.containsAll(listOf("androidx.media3.session.MediaLibraryService", "android.media.browse.MediaBrowserService")))
        val meta = parse("android/app/src/main/AndroidManifest.xml").getElementsByTagName("meta-data").let { list -> (0 until list.length).map { list.item(it) as Element } }
        assertEquals("@xml/automotive_app_desc", meta.single { it.getAttribute("android:name") == "com.google.android.gms.car.application" }.getAttribute("android:resource"))
        val uses = parse("android/app/src/main/res/xml/automotive_app_desc.xml").documentElement.children("uses").map { it.getAttribute("name") }
        assertEquals(listOf("media"), uses)
    }

    @Test
    fun theOnlyProvidersAreArtworkAndThePrivateWatchappFile() {
        val providers = listOf("android/playback/src/main/AndroidManifest.xml", "android/app/src/main/AndroidManifest.xml")
            .flatMap { path -> parse(path).getElementsByTagName("provider").let { list -> (0 until list.length).map { list.item(it) as Element } } }
        assertEquals(setOf("app.hocket.playback.ArtworkProvider", "app.hocket.playback.pebble.PebbleFileProvider"), providers.map { it.getAttribute("android:name") }.toSet())
        // Exported so controllers can open artwork; ArtworkProvider itself checks the caller.
        val artwork = providers.single { it.getAttribute("android:name") == "app.hocket.playback.ArtworkProvider" }
        assertFalse(artwork.hasAttribute("android:grantUriPermissions"))
        assertFalse(artwork.hasAttribute("android:writePermission"))
        // The bundled watchapp: never exported, reachable only through a per-intent read grant, and
        // serving nothing but its own cache directory.
        val pebble = providers.single { it.getAttribute("android:name") == "app.hocket.playback.pebble.PebbleFileProvider" }
        assertEquals("false", pebble.getAttribute("android:exported"))
        assertFalse(pebble.hasAttribute("android:writePermission"))
        val paths = parse("android/playback/src/main/res/xml/pebble_paths.xml").documentElement.let { root ->
            (0 until root.childNodes.length).map { root.childNodes.item(it) }.filterIsInstance<Element>()
        }
        assertEquals(listOf("cache-path:pebble/"), paths.map { it.tagName + ":" + it.getAttribute("path") })
    }
}
