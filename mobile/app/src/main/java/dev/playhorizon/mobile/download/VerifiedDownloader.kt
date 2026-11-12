package dev.playhorizon.mobile.download

import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.nio.file.Files
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.StandardCopyOption
import java.security.DigestInputStream
import java.security.MessageDigest

/**
 * Blocking downloader. Call from a worker thread, never from the UI thread.
 * A .part file is retained after network errors so the next attempt can resume.
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
        expectedSize: Long? = null,
        onProgress: (Progress) -> Unit = {},
    ): File {
        val normalizedAlgorithm = algorithm.uppercase()
        require(normalizedAlgorithm == "SHA-1" || normalizedAlgorithm == "SHA-256") {
            "Unsupported checksum algorithm"
        }
        val digestLength = if (normalizedAlgorithm == "SHA-1") 40 else 64
        require(expectedDigest.matches(Regex("[a-fA-F0-9]{$digestLength}"))) {
            "Invalid expected checksum"
        }
        require(expectedSize == null || expectedSize >= 0L) { "Invalid expected file size" }

        var currentUrl = URL(urlText)
        require(currentUrl.protocol == "https" && isAllowedHost(currentUrl.host)) {
            "Download host is not trusted: ${currentUrl.host}"
        }
        destination.parentFile?.let { parent ->
            if (!parent.exists() && !parent.mkdirs()) error("Cannot create download directory")
        }
        val part = File(destination.parentFile, destination.name + ".part")

        // Recover cleanly if the previous process died after downloading the whole file.
        if (part.isFile && expectedSize != null && part.length() == expectedSize) {
            if (matchesDigest(part, normalizedAlgorithm, expectedDigest)) {
                finalizeFile(part, destination)
                return destination
            }
            if (!part.delete()) error("Cannot discard invalid partial download")
        } else if (part.exists() && expectedSize != null && part.length() > expectedSize) {
            if (!part.delete()) error("Cannot discard oversized partial download")
        }

        var connection: HttpURLConnection? = null
        try {
            var redirects = 0
            while (true) {
                connection = (currentUrl.openConnection() as HttpURLConnection).apply {
                    connectTimeout = connectTimeoutMs
                    readTimeout = readTimeoutMs
                    instanceFollowRedirects = false
                    setRequestProperty("Accept-Encoding", "identity")
                    if (part.exists() && part.length() > 0L) {
                        setRequestProperty("Range", "bytes=${part.length()}-")
                    }
                    connect()
                }
                val code = connection.responseCode
                if (code !in 300..399) break
                if (redirects++ >= 5) error("Too many download redirects")
                val location = connection.getHeaderField("Location") ?: error("Redirect without Location")
                val nextUrl = URL(currentUrl, location)
                require(nextUrl.protocol == "https" && isAllowedHost(nextUrl.host)) {
                    "Redirect to untrusted host: ${nextUrl.host}"
                }
                connection.disconnect()
                connection = null
                currentUrl = nextUrl
            }

            val active = connection ?: error("No download connection")
            val code = active.responseCode
            val append = code == HttpURLConnection.HTTP_PARTIAL && part.exists() && part.length() > 0L
            if (code != HttpURLConnection.HTTP_OK && !append) {
                error("Download failed: HTTP $code")
            }
            if (append) {
                val range = active.getHeaderField("Content-Range") ?: run {
                    part.delete()
                    error("Missing Content-Range")
                }
                if (!range.startsWith("bytes ${part.length()}-")) {
                    part.delete()
                    error("Invalid resume range")
                }
            } else if (part.exists() && !part.delete()) {
                error("Cannot restart incomplete download")
            }

            val existing = if (append) part.length() else 0L
            val responseLength = active.getHeaderFieldLong("Content-Length", -1L)
            val total = if (responseLength >= 0L) existing + responseLength else -1L
            active.inputStream.use { input ->
                FileOutputStream(part, append).use { output ->
                    val buffer = ByteArray(64 * 1024)
                    var downloaded = existing
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        output.write(buffer, 0, count)
                        downloaded += count
                        onProgress(Progress(downloaded, total))
                    }
                    output.fd.sync()
                }
            }

            if (expectedSize != null && part.length() != expectedSize) {
                val actualSize = part.length()
                if (actualSize > expectedSize) part.delete()
                error("Incomplete download: expected $expectedSize bytes, got $actualSize; partial file kept for resume")
            }
            if (!matchesDigest(part, normalizedAlgorithm, expectedDigest)) {
                part.delete()
                error("Checksum mismatch for downloaded file")
            }
            finalizeFile(part, destination)
            return destination
        } finally {
            connection?.disconnect()
        }
    }

    private fun matchesDigest(file: File, algorithm: String, expected: String): Boolean {
        val digest = MessageDigest.getInstance(algorithm)
        DigestInputStream(file.inputStream().buffered(), digest).use { input ->
            val buffer = ByteArray(64 * 1024)
            while (input.read(buffer) != -1) { /* stream through checksum */ }
        }
        val actual = digest.digest().joinToString("") { "%02x".format(it) }
        return actual.equals(expected, ignoreCase = true)
    }

    private fun finalizeFile(part: File, destination: File) {
        try {
            Files.move(
                part.toPath(),
                destination.toPath(),
                StandardCopyOption.REPLACE_EXISTING,
                StandardCopyOption.ATOMIC_MOVE,
            )
        } catch (_: AtomicMoveNotSupportedException) {
            // The file is already fully verified; same-directory replacement is safe as fallback.
            Files.move(part.toPath(), destination.toPath(), StandardCopyOption.REPLACE_EXISTING)
        }
    }

    private fun isAllowedHost(host: String): Boolean = host.lowercase() in setOf(
        "piston-meta.mojang.com",
        "piston-data.mojang.com",
        "launcher.mojang.com",
        "libraries.minecraft.net",
        "resources.download.minecraft.net",
        "github.com",
        "objects.githubusercontent.com",
        "release-assets.githubusercontent.com",
    )
}
