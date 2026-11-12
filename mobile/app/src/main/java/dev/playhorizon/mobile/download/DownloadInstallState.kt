package dev.playhorizon.mobile.download

/**
 * Shared state model for the future install screen. A download is not installed
 * until verification and the final validation stage have both succeeded.
 */
enum class InstallPhase {
    NOT_INSTALLED,
    RESOLVING_METADATA,
    DOWNLOADING,
    VERIFYING,
    PREPARING,
    INSTALLED,
    CANCELLED,
    FAILED,
}

data class InstallProgress(
    val phase: InstallPhase,
    val completedBytes: Long = 0,
    val totalBytes: Long? = null,
    val currentFile: String? = null,
    val error: String? = null,
) {
    val fraction: Float?
        get() = totalBytes?.takeIf { it > 0 }?.let {
            (completedBytes.toDouble() / it).coerceIn(0.0, 1.0).toFloat()
        }
}
