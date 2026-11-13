package dev.playhorizon.mobile.download

import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.DigestInputStream
import java.security.MessageDigest
import java.util.zip.ZipInputStream
import java.util.concurrent.TimeUnit

/**
 * Installs the full Android ARM64 OpenJDK 25 (java + javac) required by
 * horizon-prepare. The archive is published by this repository's android-jdk25
 * workflow; no desktop/Linux JDK is accepted.
 */
class AndroidJdkInstaller(
    private val downloader: VerifiedDownloader = VerifiedDownloader(),
) {
    data class Progress(val downloaded: Long, val total: Long)

    fun install(
        filesDir: File,
        isCancelled: () -> Boolean = { false },
        onProgress: (Progress) -> Unit = {},
        onMessage: (String) -> Unit = {},
    ): File {
        val manifestUrl = RELEASE_BASE + "runtime.json"
        onMessage("Verificando o manifesto do OpenJDK 25 ARM64...")
        val manifest = JSONObject(fetchText(manifestUrl))
        require(manifest.optInt("schema") == 1) { "Manifesto do runtime Android desconhecido" }
        require(manifest.optInt("java_major") == 25) { "O runtime não é Java 25" }
        require(manifest.optString("os") == "android") { "Runtime de sistema operacional incorreto" }
        require(manifest.optString("architecture") == "aarch64") { "Runtime não é ARM64" }
        val archiveName = manifest.getString("archive")
        require(archiveName == ARCHIVE_NAME) { "Nome de arquivo do runtime não permitido" }
        val sha256 = manifest.getString("sha256").lowercase()
        require(sha256.matches(Regex("[a-f0-9]{64}"))) { "SHA-256 inválido no manifesto" }

        val runtimeRoot = File(filesDir, "runtimes/horizon-jdk25").canonicalFile
        val javaHome = File(runtimeRoot, "jdk")
        val java = File(javaHome, "bin/java")
        val javac = File(javaHome, "bin/javac")
        if (java.isFile && javac.isFile) {
            val javaFailure = java25Failure(java, "java")
            val javacFailure = java25Failure(javac, "javac")
            if (javaFailure == null && javacFailure == null) {
                onMessage("OpenJDK 25 ARM64 já está instalado e validado.")
                return javaHome
            }
            onMessage("Runtime Java existente inválido; será reinstalado. " +
                listOfNotNull(javaFailure, javacFailure).joinToString(" | "))
        }

        val archive = File(filesDir, "cache/$ARCHIVE_NAME")
        onMessage("Baixando o JDK 25 completo para Android ARM64...")
        downloader.download(
            urlText = RELEASE_BASE + archiveName,
            destination = archive,
            algorithm = "SHA-256",
            expectedDigest = sha256,
        ) { p ->
            if (isCancelled()) throw InterruptedException("Instalação do Java cancelada")
            onProgress(Progress(p.downloaded, p.total))
        }

        if (isCancelled()) throw InterruptedException("Instalação do Java cancelada")
        runtimeRoot.parentFile?.let { check(it.mkdirs() || it.isDirectory) }
        val staging = File(runtimeRoot.parentFile, ".horizon-jdk25-installing")
        if (staging.exists()) staging.deleteRecursively()
        check(staging.mkdirs()) { "Não foi possível criar a pasta temporária do Java" }
        try {
            extractSafely(archive, staging, isCancelled, onMessage)
            val stagedHome = File(staging, "jdk")
            val stagedJava = File(stagedHome, "bin/java")
            val stagedJavac = File(stagedHome, "bin/javac")
            require(stagedJava.isFile && stagedJavac.isFile) {
                "O arquivo baixado não contém java e javac para Android ARM64"
            }
            stagedJava.setExecutable(true, true)
            stagedJavac.setExecutable(true, true)
            val javaFailure = java25Failure(stagedJava, "java")
            val javacFailure = java25Failure(stagedJavac, "javac")
            require(javaFailure == null && javacFailure == null) {
                "O runtime instalado não executa como Java 25. " +
                    listOfNotNull(javaFailure, javacFailure).joinToString(" | ")
            }

            if (runtimeRoot.exists()) check(runtimeRoot.deleteRecursively()) {
                "Não foi possível substituir a instalação antiga do Java"
            }
            check(runtimeRoot.mkdirs()) { "Não foi possível criar a pasta final do Java" }
            check(stagedHome.renameTo(javaHome)) { "Não foi possível ativar o OpenJDK 25" }
            return javaHome
        } finally {
            staging.deleteRecursively()
        }
    }

    private fun extractSafely(
        archive: File,
        destination: File,
        isCancelled: () -> Boolean,
        onMessage: (String) -> Unit,
    ) {
        var entries = 0
        var expanded = 0L
        ZipInputStream(BufferedInputStream(FileInputStream(archive))).use { zip ->
            while (true) {
                if (isCancelled()) throw InterruptedException("Extração do Java cancelada")
                val entry = zip.nextEntry ?: break
                entries++
                require(entries <= MAX_ENTRIES) { "Arquivo do Java contém entradas demais" }
                val name = entry.name.replace(92.toChar(), '/').trimEnd('/')
                require(name.isNotBlank() && !name.startsWith("/") &&
                    name.split('/').none { it == ".." || it.isEmpty() }) {
                    "Caminho inseguro no arquivo do Java"
                }
                val target = File(destination, name).canonicalFile
                require(target.toPath().startsWith(destination.canonicalFile.toPath())) {
                    "Entrada do Java escapou da pasta de instalação"
                }
                if (entry.isDirectory) {
                    check(target.mkdirs() || target.isDirectory) { "Não foi possível criar pasta do Java" }
                } else {
                    check(target.parentFile?.let { it.mkdirs() || it.isDirectory } == true) {
                        "Não foi possível criar diretório do Java"
                    }
                    FileOutputStream(target).use { output ->
                        val buffer = ByteArray(64 * 1024)
                        while (true) {
                            if (isCancelled()) throw InterruptedException("Extração do Java cancelada")
                            val count = zip.read(buffer)
                            if (count < 0) break
                            expanded += count
                            require(expanded <= MAX_EXPANDED_BYTES) { "Arquivo do Java excede o limite de extração" }
                            output.write(buffer, 0, count)
                        }
                        output.fd.sync()
                    }
                }
                zip.closeEntry()
                if (entries % 500 == 0) onMessage("Extraindo runtime Java: $entries arquivos...")
            }
        }
        require(entries > 0) { "Arquivo do Java vazio" }
    }

    /**
     * Runs the Android JDK launchers and preserves their actual linker/runtime
     * diagnostics. libjli.so lives in lib/jli on this JDK layout; omitting that
     * directory from LD_LIBRARY_PATH can make java fail before printing a version.
     */
    private fun java25Failure(executable: File, tool: String): String? {
        return try {
            val home = executable.parentFile?.parentFile
                ?: return "caminho do $tool inválido: ${executable.absolutePath}"
            val builder = ProcessBuilder(executable.absolutePath, "-version")
                .redirectErrorStream(true)
            builder.environment()["JAVA_HOME"] = home.absolutePath
            val inherited = builder.environment()["LD_LIBRARY_PATH"].orEmpty()
                .split(File.pathSeparator)
                .filter { it.isNotBlank() }
            val libraryPaths = listOf(
                File(home, "lib/jli").absolutePath,
                File(home, "lib").absolutePath,
                File(home, "lib/server").absolutePath,
            ) + inherited
            builder.environment()["LD_LIBRARY_PATH"] = libraryPaths.distinct()
                .joinToString(File.pathSeparator)

            val process = builder.start()
            val output = StringBuilder()
            val reader = Thread {
                process.inputStream.bufferedReader().useLines { lines ->
                    lines.forEach { line -> output.append(line).append('\n') }
                }
            }.apply { name = "horizon-${tool}-version-check"; isDaemon = true; start() }

            if (!process.waitFor(20, TimeUnit.SECONDS)) {
                process.destroyForcibly()
                reader.join(1_000)
                return "$tool -version excedeu o limite de 20 segundos"
            }
            reader.join(2_000)
            val versionOutput = output.toString().trim()
            if (process.exitValue() != 0) {
                return "$tool -version terminou com código ${process.exitValue()}: " +
                    versionOutput.ifBlank { "nenhuma saída; verifique linker/execução ARM64" }
            }
            val isExpectedVersion = when (tool) {
                "java" -> Regex("""(?m)^(?:openjdk|java) version ["']?25(?:[-+._][^"'\s]*)?["']?""")
                    .containsMatchIn(versionOutput)
                "javac" -> Regex("""(?m)^javac\s+25(?:[-+._][^\s]*)?""")
                    .containsMatchIn(versionOutput)
                else -> false
            }
            if (isExpectedVersion) null
            else "$tool não informou versão 25; saída: " +
                versionOutput.ifBlank { "vazia" }
        } catch (error: Throwable) {
            "$tool não iniciou (${error.javaClass.simpleName}): " +
                (error.message ?: "sem detalhe")
        }
    }

    private fun fetchText(urlText: String, redirects: Int = 0): String {
        require(redirects <= 5) { "Redirecionamentos demais ao consultar runtime" }
        val url = URL(urlText)
        require(url.protocol == "https" && url.host.lowercase() in ALLOWED_HOSTS) {
            "Host não confiável para runtime Java"
        }
        val connection = url.openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 20_000
            connection.readTimeout = 30_000
            connection.instanceFollowRedirects = false
            val code = connection.responseCode
            if (code in 300..399) {
                val location = connection.getHeaderField("Location")
                    ?: error("Redirecionamento sem destino")
                return fetchText(URL(url, location).toString(), redirects + 1)
            }
            require(code == HttpURLConnection.HTTP_OK) { "Falha ao consultar runtime Java: HTTP $code" }
            return connection.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        } finally {
            connection.disconnect()
        }
    }

    companion object {
        private const val RELEASE_BASE =
            "https://github.com/mikael8367/Mine-horizon-rust-mobile/releases/download/android-jdk25-arm64/"
        private const val ARCHIVE_NAME = "horizon-jdk25-android-arm64.zip"
        private const val MAX_ENTRIES = 100_000
        private const val MAX_EXPANDED_BYTES = 2L * 1024L * 1024L * 1024L
        private val ALLOWED_HOSTS = setOf("github.com", "release-assets.githubusercontent.com")
    }
}
