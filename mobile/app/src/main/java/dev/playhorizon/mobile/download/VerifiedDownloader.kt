package dev.playhorizon.mobile.download

import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/**
 * Small blocking downloader. Call from a worker thread, never from the UI thread.
 * A .part file is retained after network errors so the next attempt can resume.
 *
 * The expected digest must come from trusted Mojang version metadata or a
 * verified Horizon release manifest; never accept an arbitrary checksum from UI.
 */
class VerifiedDownloader(
    private val connectTimeoutMs: Int = 15_000,
    private val readTimeoutMs: Int = 30_000,
) {
    data class Progress(val downloaded: Long, val total: Long)

    fun download(
        urlText: String,
        destination: File,
        algorithm: String,
        expectedDigest: String,
        onProgress: (Progress) -> Unit = {},
    ): File {
        require(algorithm.equals("SHA-1", true) || algorithm.equals("SHA-256", true)) {
            "Unsupported checksum algorithm"
        }
        require(expectedDigest.matches(Regex(if (algorithm.equals("SHA-1", true)) "[a-fA-F0-9]{40}" else "[a-fA-F0-9]{64}"))) {
            "Invalid expected checksum"
        }
        val initialUrl = URL(urlText)
        require(initialUrl.protocol == "https" && isAllowedHost(initialUrl.host)) {
            "Download host is not trusted: ${initialUrl.host}"
        }

        destination.parentFile?.let { parent ->
            if (!parent.exists() && !parent.mkdirs()) error("Cannot create download directory")
        }
        val part = File(destination.parentFile, destination.name + ".part")
        var connection: HttpURLConnection? = null
        try {
            connection = (initialUrl.openConnection() as HttpURLConnection).apply {
                connectTimeout = connectTimeoutMs
                readTimeout = readTimeoutMs
                instanceFollowRedirects = false
                setRequestProperty("Accept-Encoding", "identity")
                if (part.exists() && part.length() > 0L) {
                    setRequestProperty("Range", "bytes=${part.length()}-")
                }
                connect()
            }

            // Follow only HTTPS redirects to explicitly allowed distribution hosts.
            var redirects = 0
            while (connection.responseCode in 300..399 && redirects++ < 5) {
                val location = connection.getHeaderField("Location") ?: error("Redirect without Location")
                val next = URL(initialUrl, location)
                require(next.protocol == "https" && isAllowedHost(next.host)) {
                    "Redirect to untrusted host: ${next.host}"
                }
                connection.disconnect()
                connection = (next.openConnection() as HttpURLConnection).apply {
                    connectTimeout = connectTimeoutMs
                    readTimeout = readTimeoutMs
                    instanceFollowRedirects = false
                    setRequestProperty("Accept-Encoding", "identity")
                    if (part.exists() && part.length() > 0L) {
                        setRequestProperty("Range", "bytes=${part.length()}-")
                    }
                    connect()
                }
            }

            val code = connection.responseCode
            val append = code == HttpURLConnection.HTTP_PARTIAL && part.exists()
            if (code != HttpURLConnection.HTTP_OK && !append) {
                error("Download failed: HTTP $code")
            }
            if (code == HttpURLConnection.HTTP_PARTIAL) {
                val range = connection.getHeaderField("Content-Range") ?: error("Missing Content-Range")
                require(range.startsWith("bytes ${part.length()}-")) { "Invalid resume range" }
            } else if (part.exists()) {
                // Server ignored Range; restart cleanly instead of duplicating bytes.
                part.delete()
            }

            val existing = if (append) part.length() else 0L
            val responseLength = connection.getHeaderFieldLong("Content-Length", -1L)
            val total = if (responseLength >= 0) existing + responseLength else -1L
            connection.inputStream.use { input ->
                FileOutputStream(part, append).use { output ->
                    val buffer = ByteArray(DEFAULT_BUFFER_SIZE * 8)
                    var count: Int
                    var downloaded = existing
                    while (input.read(buffer).also { count = it } != -1) {
                        output.write(buffer, 0, count)
                        downloaded += count
                        onProgress(Progress(downloaded, total))
                    }
                    output.fd.sync()
                }
            }

            val digest = MessageDigest.getInstance(algorithm.uppercase().replace("-", "")).digest(part.readBytes())
                .joinToString("") { "%02x".format(it) }
            require(digest.equals(expectedDigest, ignoreCase = true)) {
                "Checksum mismatch: expected $expectedDigest, got $digest"
            }
            if (destination.exists() && !destination.delete()) error("Cannot replace destination")
            if (!part.renameTo(destination)) error("Cannot finalize verified download")
            return destination
        } finally {
            connection?.disconnect()
        }
    }

    private fun isAllowedHost(host: String): Boolean {
        val normalized = host.lowercase()
        return normalized == "piston-meta.mojang.com" ||
            normalized == "piston-data.mojang.com" ||
            normalized == "launcher.mojang.com" ||
            normalized == "libraries.minecraft.net" ||
            normalized == "resources.download.minecraft.net" ||
            normalized == "github.com" ||
            normalized == "objects.githubusercontent.com" ||
            normalized == "release-assets.githubusercontent.com"
    }
}
