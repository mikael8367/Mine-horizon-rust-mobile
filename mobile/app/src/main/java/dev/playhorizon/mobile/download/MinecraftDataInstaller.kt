package dev.playhorizon.mobile.download

import java.io.File
import java.security.DigestInputStream
import java.security.MessageDigest
import java.util.concurrent.CancellationException

/**
 * Installs the pinned Minecraft inputs into app-private storage.
 * Run on an IO worker. The caller must gate this operation behind the app's
 * Microsoft sign-in and Minecraft: Java Edition entitlement flow.
 */
class MinecraftDataInstaller(
    private val downloader: VerifiedDownloader = VerifiedDownloader(),
) {
    data class Progress(
        val completedBytes: Long,
        val totalBytes: Long,
        val currentPath: String,
        val fileProgress: VerifiedDownloader.Progress,
    )

    fun install(
        plan: MojangDownloadPlan.Plan,
        gameDirectory: File,
        isCancelled: () -> Boolean = { false },
        onProgress: (Progress) -> Unit = {},
    ): File {
        require(plan.version == "26.3") { "Unsupported Horizon game-data version" }
        if (!gameDirectory.exists() && !gameDirectory.mkdirs()) {
            error("Cannot create Minecraft data directory")
        }
        val root = gameDirectory.canonicalFile
        var completed = 0L

        for (artifact in plan.artifacts) {
            if (isCancelled()) throw CancellationException("Minecraft download cancelled")
            val relative = File(artifact.relativePath)
            require(!relative.isAbsolute && relative.path.split(Regex("[/\\\\]+")).none { it == ".." }) {
                "Unsafe artifact path: ${artifact.relativePath}"
            }
            val target = File(root, artifact.relativePath).canonicalFile
            require(target.toPath().startsWith(root.toPath())) { "Artifact escaped game directory" }
            target.parentFile?.let {
                if (!it.exists() && !it.mkdirs()) error("Cannot create folder for ${artifact.relativePath}")
            }

            if (target.isFile && target.length() == artifact.size && hasSha1(target, artifact.sha1)) {
                completed += artifact.size
                onProgress(Progress(completed, plan.totalBytes, artifact.relativePath,
                    VerifiedDownloader.Progress(artifact.size, artifact.size)))
                continue
            }

            downloader.download(
                urlText = artifact.url,
                destination = target,
                algorithm = "SHA-1",
                expectedDigest = artifact.sha1,
                expectedSize = artifact.size,
            ) { fileProgress ->
                if (isCancelled()) throw CancellationException("Minecraft download cancelled")
                onProgress(Progress(
                    completed + fileProgress.downloaded,
                    plan.totalBytes,
                    artifact.relativePath,
                    fileProgress,
                ))
            }
            completed += artifact.size
        }
        return root
    }

    private fun hasSha1(file: File, expected: String): Boolean {
        val digest = MessageDigest.getInstance("SHA-1")
        DigestInputStream(file.inputStream().buffered(), digest).use { input ->
            val buffer = ByteArray(64 * 1024)
            while (input.read(buffer) != -1) { /* stream checksum */ }
        }
        val actual = digest.digest().joinToString("") { "%02x".format(it) }
        return actual.equals(expected, ignoreCase = true)
    }
}
