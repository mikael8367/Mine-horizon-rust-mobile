package dev.playhorizon.mobile.download

import android.content.Context
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.util.concurrent.CancellationException
import java.util.concurrent.TimeUnit

/** Runs the real Rust horizon-prepare executable against the player's verified Mojang files. */
class VanillaResourcePreparer(private val context: Context) {
    data class Progress(val fraction: Double?, val message: String)

    fun prepare(
        isCancelled: () -> Boolean = { false },
        onProgress: (Progress) -> Unit = {},
    ): File {
        fun checkCancelled() {
            if (isCancelled() || Thread.currentThread().isInterrupted) {
                throw CancellationException("Preparação vanilla cancelada")
            }
        }
        checkCancelled()
        val appFiles = context.filesDir
        val root = File(appFiles, "horizon-prepare-root")
        val output = File(appFiles, ".local/horizon-client/26.3")
        val game = File(appFiles, "minecraft/26.3")
        val versionJson = File(game, "versions/26.3/26.3.json")
        val clientJar = File(game, "versions/26.3/26.3.jar")
        val serverJar = File(game, "versions/26.3/26.3-server.jar")
        val libraries = File(game, "libraries")
        val assets = File(game, "assets")
        require(versionJson.isFile && clientJar.isFile && serverJar.isFile &&
            libraries.isDirectory && File(assets, "indexes").isDirectory &&
            File(assets, "objects").isDirectory) {
            "Os arquivos oficiais estão incompletos. Baixe novamente os arquivos do Minecraft antes de preparar."
        }
        val jdkHome = AndroidJdkInstaller().install(
            filesDir = appFiles,
            isCancelled = ::isCancelled,
            onProgress = { p ->
                val total = p.total.takeIf { it > 0L }
                onProgress(Progress(
                    if (total == null) null else p.downloaded.toDouble() / total,
                    "Baixando OpenJDK 25 ARM64: ${p.downloaded / 1_048_576} MB" +
                        (total?.let { " / ${it / 1_048_576} MB" } ?: ""),
                ))
            },
            onMessage = { onProgress(Progress(null, it)) },
        )
        checkCancelled()
        installSupportFiles(root)

        val preparer = File(appFiles, "bin/horizon-prepare")
        preparer.parentFile?.mkdirs()
        val stagedPreparer = File(preparer.parentFile, "horizon-prepare.new")
        context.assets.open("bin/horizon-prepare").use { input ->
            FileOutputStream(stagedPreparer).use { outputStream -> input.copyTo(outputStream) }
        }
        stagedPreparer.setExecutable(true, true)
        if (preparer.exists()) check(preparer.delete()) {
            "Não foi possível atualizar o executável de preparação"
        }
        check(stagedPreparer.renameTo(preparer) && preparer.setExecutable(true, true)) {
            "Não foi possível ativar o executável de preparação"
        }
        check(preparer.canExecute()) { "O executável horizon-prepare não pode ser iniciado" }
        require(File(root, "research-lock.json").isFile &&
            File(root, "mods/horizon/src/test/java/dev/horizonlab/parity").isDirectory &&
            File(root, "crates/horizon-prepare/src/exporters.rs").isFile) {
            "Arquivos de suporte do preparador não foram incluídos no app"
        }

        output.parentFile?.mkdirs()
        val command = listOf(
            preparer.absolutePath,
            "--root", root.absolutePath,
            "--output", output.absolutePath,
            "--version-json", versionJson.absolutePath,
            "--client-jar", clientJar.absolutePath,
            "--server-jar", serverJar.absolutePath,
            "--libraries-dir", libraries.absolutePath,
            "--assets-dir", assets.absolutePath,
            "--java-home", jdkHome.absolutePath,
            "--complete",
            "--progress", "json",
        )
        val builder = ProcessBuilder(command)
            .directory(root)
            .redirectErrorStream(true)
        builder.environment()["JAVA_HOME"] = jdkHome.absolutePath
        builder.environment()["PATH"] = File(jdkHome, "bin").absolutePath +
            File.pathSeparator + (builder.environment()["PATH"] ?: "")
        builder.environment().remove("JAVA_TOOL_OPTIONS")
        builder.environment().remove("_JAVA_OPTIONS")
        builder.environment().remove("JDK_JAVA_OPTIONS")
        val process = builder.start()

        val reader = Thread {
            process.inputStream.bufferedReader().useLines { lines ->
                lines.forEach { line ->
                    val parsed = try { JSONObject(line) } catch (_: Throwable) { null }
                    when {
                        parsed?.has("fraction") == true -> onProgress(
                            Progress(parsed.optDouble("fraction").coerceIn(0.0, 1.0), "Preparando recursos vanilla...")
                        )
                        parsed?.has("message") == true -> onProgress(
                            Progress(null, parsed.optString("message"))
                        )
                        parsed?.has("error") == true -> onProgress(
                            Progress(null, "Erro: ${parsed.optString("error")}")
                        )
                        line.isNotBlank() -> onProgress(Progress(null, line))
                    }
                }
            }
        }.apply { name = "horizon-prepare-output"; isDaemon = true; start() }

        try {
            while (!process.waitFor(250, TimeUnit.MILLISECONDS)) checkCancelled()
            reader.join(2_000)
            check(process.exitValue() == 0) {
                "horizon-prepare falhou (código ${process.exitValue()}). Confira as mensagens acima."
            }
        } catch (error: Throwable) {
            process.destroy()
            if (process.isAlive) process.destroyForcibly()
            throw error
        }

        checkCancelled()
        val manifest = File(output, "horizon-resources.json")
        require(manifest.isFile && File(output, "horizon-prepare.json").isFile) {
            "A preparação terminou sem os manifestos obrigatórios do Horizon"
        }
        val checker = ProcessBuilder(
            preparer.absolutePath, "--root", root.absolutePath,
            "--output", output.absolutePath, "--check",
        ).directory(root).redirectErrorStream(true).start()
        val checkOutput = checker.inputStream.bufferedReader().use { it.readText() }
        require(checker.waitFor() == 0) {
            "A validação final dos recursos falhou: $checkOutput"
        }
        onProgress(Progress(1.0, "Recursos vanilla preparados e validados para Minecraft 26.3."))
        return output
    }

    private fun installSupportFiles(destination: File) {
        val lock = File(destination, "research-lock.json")
        val parity = File(destination, "mods/horizon/src/test/java/dev/horizonlab/parity")
        val exporter = File(destination, "crates/horizon-prepare/src/exporters.rs")
        if (lock.isFile && parity.isDirectory && exporter.isFile) return
        if (destination.exists()) check(destination.deleteRecursively()) {
            "Não foi possível reparar o pacote de suporte incompleto"
        }
        check(destination.mkdirs() || destination.isDirectory) {
            "Não foi possível criar a pasta de suporte do preparador"
        }
        copyAssetTree("prepare-root", destination)
        require(lock.isFile && parity.isDirectory && exporter.isFile) {
            "Pacote de suporte vanilla incompleto"
        }
    }

    private fun copyAssetTree(assetPath: String, destination: File) {
        val children = context.assets.list(assetPath).orEmpty()
        if (children.isEmpty()) {
            destination.parentFile?.mkdirs()
            context.assets.open(assetPath).use { input ->
                FileOutputStream(destination).use { output -> input.copyTo(output) }
            }
            return
        }
        check(destination.mkdirs() || destination.isDirectory) {
            "Não foi possível criar pasta de suporte"
        }
        for (child in children) {
            require(child != "." && child != ".." && !child.contains('/')) {
                "Nome inseguro no pacote de suporte"
            }
            copyAssetTree("$assetPath/$child", File(destination, child))
        }
    }
}
