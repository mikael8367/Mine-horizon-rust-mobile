package dev.playhorizon.mobile.download

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/**
 * Resolves the exact Horizon-pinned Minecraft 26.3 inputs from Mojang metadata.
 * This does not authenticate an account or itself enable Play.
 */
class MojangDownloadPlan {
    data class Artifact(
        val relativePath: String,
        val url: String,
        val sha1: String,
        val size: Long,
        val role: String,
    )

    data class Plan(
        val version: String,
        val artifacts: List<Artifact>,
    ) {
        val totalBytes: Long get() = artifacts.sumOf { it.size }
    }

    companion object {
        private const val VERSION = "26.3"
        private const val MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
        private const val CLIENT_SHA1 = "e877b6a07acd633fb3bb475002175cec036e7b87"
        private const val SERVER_SHA1 = "33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c"
        private val VERSION_JSON_SHA1 = setOf(
            "11f1aefe473eefba0af62e3286c33b692807b2eb",
            "4fe1aa1ef8da1cb95c5bad1fb98890ca56dd8ca3",
            "702fe59163c6ee6578607daa85811d9bc9c7cc40",
            "61fff1316bd50312624a858bbb5eb18fa45208ed",
            "5df1795e16eea19d75b923d66ecca9ce183353c5",
            "80608ff6753705b6a82e2cb86a5937d4d233abd4",
            "9f67e10ed013deeaad20ac3c800652765d493408"
        ).filter { it.matches(Regex("[a-f0-9]{40}")) }.toSet()
        private val ASSET_INDEX_SHA1 = setOf(
            "abfaa525f923f807df8b4e4d29c1b5e3a104adbe",
            "1e4e4a68dece1c0f51b8f93b45da8fe8da51bcd3",
            "0c8f43b28f837869c1e54a6aeb22c396a1548946",
            "917ff6d74b9f635c46bac0d28857206bbcc1ffe4",
            "331e5c80937c9cd79647ab34bdc1cbb5ef09f456",
            "e99f9973047785b5a9d52eaf72020b97545053c9",
        )
    }

    fun resolve(): Plan {
        val manifest = JSONObject(fetchText(MANIFEST_URL))
        val versions = manifest.getJSONArray("versions")
        var versionUrl: String? = null
        for (i in 0 until versions.length()) {
            val item = versions.getJSONObject(i)
            if (item.optString("id") == VERSION) {
                versionUrl = item.optString("url").takeIf { it.isNotBlank() }
                break
            }
        }
        val metadataUrl = versionUrl ?: error("Minecraft $VERSION was not found in Mojang's version manifest")
        requireTrustedUrl(metadataUrl)
        val versionText = fetchText(metadataUrl)
        val versionSha1 = sha1(versionText.toByteArray(Charsets.UTF_8))
        require(versionSha1 in VERSION_JSON_SHA1) {
            "Minecraft $VERSION metadata differs from the Horizon-pinned version JSON"
        }
        val versionJson = JSONObject(versionText)
        require(versionJson.optString("id") == VERSION) { "Mojang returned unexpected game version metadata" }

        val artifacts = mutableListOf<Artifact>()
        val downloads = versionJson.getJSONObject("downloads")
        artifacts += artifact(downloads.getJSONObject("client"), "versions/$VERSION/$VERSION.jar", "client", CLIENT_SHA1)
        artifacts += artifact(downloads.getJSONObject("server"), "versions/$VERSION/$VERSION-server.jar", "server", SERVER_SHA1)

        val libraries = versionJson.optJSONArray("libraries")
        if (libraries != null) {
            for (i in 0 until libraries.length()) {
                val library = libraries.getJSONObject(i)
                val libraryArtifact = library.optJSONObject("downloads")?.optJSONObject("artifact") ?: continue
                val libraryPath = libraryArtifact.optString("path")
                if (libraryPath.isBlank()) continue
                artifacts += artifact(libraryArtifact, "libraries/$libraryPath", "library", null)
            }
        }

        val assetIndex = versionJson.getJSONObject("assetIndex")
        val assetIndexId = assetIndex.getString("id")
        val indexArtifact = artifact(assetIndex, "assets/indexes/$assetIndexId.json", "asset-index", null)
        require(indexArtifact.sha1 in ASSET_INDEX_SHA1) {
            "The asset index is not one of the pinned Minecraft 26.3 indexes"
        }
        artifacts += indexArtifact

        val indexText = fetchText(indexArtifact.url)
        require(sha1(indexText.toByteArray(Charsets.UTF_8)) == indexArtifact.sha1) {
            "Minecraft asset index checksum mismatch"
        }
        val indexJson = JSONObject(indexText)
        val objects = indexJson.getJSONObject("objects")
        val keys = objects.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            val obj = objects.getJSONObject(key)
            val hash = obj.getString("hash").lowercase()
            require(hash.matches(Regex("[a-f0-9]{40}"))) { "Invalid asset hash for $key" }
            val size = obj.getLong("size")
            require(size >= 0L) { "Invalid asset size for $key" }
            artifacts += Artifact(
                relativePath = "assets/objects/${hash.substring(0, 2)}/$hash",
                url = "https://resources.download.minecraft.net/${hash.substring(0, 2)}/$hash",
                sha1 = hash,
                size = size,
                role = "asset",
            )
        }
        return Plan(VERSION, artifacts.distinctBy { it.relativePath })
    }

    private fun artifact(json: JSONObject, path: String, role: String, pinnedSha1: String?): Artifact {
        val url = json.getString("url")
        requireTrustedUrl(url)
        val sha1 = json.getString("sha1").lowercase()
        require(sha1.matches(Regex("[a-f0-9]{40}"))) { "Invalid SHA-1 for $path" }
        if (pinnedSha1 != null) require(sha1 == pinnedSha1) {
            "The $role JAR does not match the pinned Horizon Minecraft $VERSION build"
        }
        val size = json.getLong("size")
        require(size > 0L) { "Invalid size for $path" }
        return Artifact(path, url, sha1, size, role)
    }

    private fun fetchText(urlText: String, redirects: Int = 0): String {
        require(redirects <= 5) { "Too many metadata redirects" }
        requireTrustedUrl(urlText)
        val connection = URL(urlText).openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 15_000
            connection.readTimeout = 30_000
            connection.instanceFollowRedirects = false
            val code = connection.responseCode
            if (code in 300..399) {
                val location = connection.getHeaderField("Location") ?: error("Metadata redirect has no location")
                val next = URL(URL(urlText), location)
                requireTrustedUrl(next.toString())
                return fetchText(next.toString(), redirects + 1)
            }
            require(code == HttpURLConnection.HTTP_OK) { "Mojang metadata request failed: HTTP $code" }
            return connection.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        } finally {
            connection.disconnect()
        }
    }

    private fun sha1(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-1").digest(bytes).joinToString("") { "%02x".format(it) }

    private fun requireTrustedUrl(value: String) {
        val url = URL(value)
        val host = url.host.lowercase()
        require(url.protocol == "https" && host in setOf(
            "piston-meta.mojang.com",
            "piston-data.mojang.com",
            "launcher.mojang.com",
            "libraries.minecraft.net",
            "resources.download.minecraft.net",
        )) { "Untrusted Mojang download URL: $value" }
    }
}
