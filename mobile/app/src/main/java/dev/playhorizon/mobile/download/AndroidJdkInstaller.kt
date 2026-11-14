package dev.playhorizon.mobile.download

import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.TimeUnit
import java.util.zip.ZipInputStream

/**
 * Installs the full Android ARM64 OpenJDK 25 (java + javac) required by
 * horizon-prepare. The archive is published by this repository's android-jdk25
 * workflow; no desktop/Linux JDK is accepted.
 *
 * Validation is strict and per tool: `java` and `javac` run separately, each
 * with `-version` and `--version`, with stdout and stderr captured apart and
 * the exit code checked. A compile-and-run self test proves the compiler before
 * the runtime is declared functional. A package whose `bin/javac` is actually
 * the `java` launcher (observed with repacked Android OpenJDK images: the
 * launcher answers `javac -version` with the java banner and exit code 0
 * because it was built without the compiled-in javac main class) is detected
 * and repaired with a `javac` shim that runs the compiler through
 * `java -m jdk.compiler/com.sun.tools.javac.Main`; the repair is only accepted
 * after the same validation (version + self test) passes through the shim.
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
            val report = validate(javaHome, filesDir)
            if (report.problems.isEmpty()) {
                onMessage("OpenJDK 25 ARM64 já está instalado e validado (java e javac testados separadamente).")
                return javaHome
            }
            if (report.javaVersionOk && report.javacIsJavaLauncher) {
                onMessage("O javac instalado é o launcher do java; escrevendo um javac funcional via 'java -m jdk.compiler'...")
                writeJavacShim(javac)
                val repaired = validate(javaHome, filesDir)
                if (repaired.problems.isEmpty()) {
                    onMessage("OpenJDK 25 ARM64 validado: o javac reparado compila e o java executa a classe compilada.")
                    return javaHome
                }
                onMessage("O reparo do javac não passou na validação; o runtime será reinstalado. " + repaired.summary())
            } else {
                onMessage("Runtime Java existente inválido; será reinstalado. " + report.summary())
            }
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
            var report = validate(stagedHome, filesDir)
            if (report.javaVersionOk && report.javacIsJavaLauncher) {
                onMessage("O pacote publicado trouxe o launcher do java no lugar do javac; escrevendo um javac funcional via 'java -m jdk.compiler'...")
                writeJavacShim(stagedJavac)
                stagedJavac.setExecutable(true, true)
                report = validate(stagedHome, filesDir)
            }
            require(report.problems.isEmpty()) {
                "O runtime baixado não é um Java 25 funcional. " + report.summary()
            }

            if (runtimeRoot.exists()) check(runtimeRoot.deleteRecursively()) {
                "Não foi possível substituir a instalação antiga do Java"
            }
            check(runtimeRoot.mkdirs()) { "Não foi possível criar a pasta final do Java" }
            check(stagedHome.renameTo(javaHome)) { "Não foi possível ativar o OpenJDK 25" }

            val activated = validate(javaHome, filesDir)
            check(activated.problems.isEmpty()) {
                runtimeRoot.deleteRecursively()
                "O runtime ativado não passou na validação final (java e javac testados separadamente). " +
                    activated.summary()
            }
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

    private class RunResult(
        val exitCode: Int?,
        val stdout: String,
        val stderr: String,
        val timedOut: Boolean,
        val startError: String?,
    )

    private class RuntimeReport {
        val problems = mutableListOf<String>()
        var javaVersionOk = false
        var javacVersionOk = false
        var javacIsJavaLauncher = false
        fun summary(): String = problems.joinToString(" | ")
    }

    /**
     * Validates one runtime home completely: required native libraries, then
     * `java` and `javac` separately (each with `-version` and `--version`,
     * stdout/stderr captured apart, exit codes checked), then a functional
     * compile-and-run self test. Never throws for a broken runtime; every
     * failure is returned as a problem naming the executable and its real
     * output.
     */
    private fun validate(home: File, workDir: File): RuntimeReport {
        val report = RuntimeReport()
        val java = File(home, "bin/java")
        val javac = File(home, "bin/javac")

        // 1. Bibliotecas nativas necessárias aos launchers e à VM.
        val requiredLibs = listOf(
            listOf("lib/jli/libjli.so", "lib/libjli.so"),
            listOf("lib/server/libjvm.so"),
            listOf("lib/modules"),
            listOf("lib/libc++_shared.so"),
        )
        for (alternatives in requiredLibs) {
            if (alternatives.none { File(home, it).isFile }) {
                report.problems.add(
                    "biblioteca nativa ausente em ${home.absolutePath}: " +
                        alternatives.joinToString(" ou ")
                )
            }
        }

        if (!java.isFile) report.problems.add("executável ausente: ${java.absolutePath}")
        if (!javac.isFile) report.problems.add("executável ausente: ${javac.absolutePath}")
        if (!java.isFile || !javac.isFile) return report

        // 2. Versões: java e javac executados separadamente, com -version e
        //    --version, sem misturar comandos nem streams.
        val javaBanner = Regex("""(?m)^(?:openjdk|java) version ["']?25(?:[-+._][^"'\s]*)?["']?""")
        val javacBanner = Regex("""(?m)^javac\s+25(?:[-+._][^\s]*)?""")

        val javaAttempts = FLAGS.map { flag -> flag to run(java, listOf(flag), home) }
        val javaOk = javaAttempts.any { (_, result) -> versionMatches(result, javaBanner) }
        if (!javaOk) {
            report.problems.add(
                "java (${java.absolutePath}) não informou versão 25. " + describeAttempts("java", javaAttempts)
            )
        } else {
            report.javaVersionOk = true
        }

        val javacAttempts = FLAGS.map { flag -> flag to run(javac, listOf(flag), home) }
        val javacOk = javacAttempts.any { (_, result) -> versionMatches(result, javacBanner) }
        if (!javacOk) {
            // Distinção essencial: o launcher do java responde a `javac -version`
            // com o banner do próprio java (código 0). Isso significa que o
            // binário publicado não é o compilador.
            report.javacIsJavaLauncher = javacAttempts.any { (_, result) -> versionMatches(result, javaBanner) }
            report.problems.add(
                if (report.javacIsJavaLauncher) {
                    "javac (${javac.absolutePath}) é o launcher do java, não o compilador: " +
                        "'javac -version' respondeu com o banner do java. " + describeAttempts("javac", javacAttempts)
                } else {
                    "javac (${javac.absolutePath}) não informou versão 25. " + describeAttempts("javac", javacAttempts)
                }
            )
        } else {
            report.javacVersionOk = true
        }

        // 3. Teste funcional: o runtime só é declarado válido se o javac compilar
        //    uma classe real e o java executá-la.
        if (report.javaVersionOk && report.javacVersionOk) {
            runSelfTest(home, java, javac, workDir, report)
        }
        return report
    }

    /**
     * Runs the Android JDK launcher with its real linker environment. stdout and
     * stderr are captured in separate buffers so a failure report shows exactly
     * which stream produced which message. libjli.so lives in lib/jli on this
     * JDK layout; omitting that directory from LD_LIBRARY_PATH can make the
     * launchers fail before printing a version.
     */
    private fun run(
        executable: File,
        args: List<String>,
        home: File,
        timeoutSeconds: Long = VERSION_TIMEOUT_SECONDS,
    ): RunResult {
        return try {
            val builder = ProcessBuilder(listOf(executable.absolutePath) + args)
            builder.environment()["JAVA_HOME"] = home.absolutePath
            val inherited = builder.environment()["LD_LIBRARY_PATH"].orEmpty()
                .split(File.pathSeparator)
                .filter { it.isNotBlank() }
            builder.environment()["LD_LIBRARY_PATH"] = (
                listOf(
                    File(home, "lib/jli").absolutePath,
                    File(home, "lib").absolutePath,
                    File(home, "lib/server").absolutePath,
                ) + inherited
                ).distinct().joinToString(File.pathSeparator)

            val process = builder.start()
            val stdout = StringBuilder()
            val stderr = StringBuilder()
            val outReader = Thread {
                runCatching {
                    process.inputStream.bufferedReader().useLines { lines ->
                        lines.forEach { line -> stdout.append(line).append('\n') }
                    }
                }
            }.apply { name = "horizon-jdk-stdout"; isDaemon = true; start() }
            val errReader = Thread {
                runCatching {
                    process.errorStream.bufferedReader().useLines { lines ->
                        lines.forEach { line -> stderr.append(line).append('\n') }
                    }
                }
            }.apply { name = "horizon-jdk-stderr"; isDaemon = true; start() }

            if (!process.waitFor(timeoutSeconds, TimeUnit.SECONDS)) {
                process.destroyForcibly()
                outReader.join(1_000)
                errReader.join(1_000)
                return RunResult(null, stdout.toString(), stderr.toString(), true, null)
            }
            outReader.join(2_000)
            errReader.join(2_000)
            RunResult(process.exitValue(), stdout.toString(), stderr.toString(), false, null)
        } catch (error: Throwable) {
            RunResult(null, "", "", false, "${error.javaClass.simpleName}: ${error.message ?: "sem detalhe"}")
        }
    }

    private fun runSelfTest(home: File, java: File, javac: File, workDir: File, report: RuntimeReport) {
        val selftest = File(workDir, ".horizon-jdk25-selftest")
        try {
            selftest.deleteRecursively()
            if (!selftest.mkdirs() && !selftest.isDirectory) {
                report.problems.add("não foi possível criar a pasta do autoteste em ${selftest.absolutePath}")
                return
            }
            val source = File(selftest, "Hello.java")
            source.writeText(
                "public class Hello {\n" +
                    "    public static void main(String[] args) {\n" +
                    "        System.out.println(\"HORIZON_JDK_SELFTEST_OK\");\n" +
                    "    }\n" +
                    "}\n",
            )
            val classes = File(selftest, "classes")
            if (!classes.mkdirs() && !classes.isDirectory) {
                report.problems.add("não foi possível criar a pasta de classes do autoteste")
                return
            }
            // -J options also exercise the launcher's -J translation on both the
            // real javac launcher and the repair shim.
            val compile = run(
                javac,
                listOf("-J-Xms8m", "-J-XX:TieredStopAtLevel=1", "-d", classes.absolutePath, source.absolutePath),
                home,
                COMPILE_TIMEOUT_SECONDS,
            )
            when {
                compile.timedOut -> report.problems.add(
                    "javac (${javac.absolutePath}) excedeu ${COMPILE_TIMEOUT_SECONDS}s compilando a classe de teste. " +
                        "stdout: ${cap(compile.stdout)}; stderr: ${cap(compile.stderr)}"
                )
                compile.startError != null -> report.problems.add(
                    "javac (${javac.absolutePath}) não iniciou o autoteste: ${compile.startError}"
                )
                compile.exitCode != 0 || !File(classes, "Hello.class").isFile -> report.problems.add(
                    "javac (${javac.absolutePath}) não compilou a classe de teste (código ${compile.exitCode}). " +
                        "stdout: ${cap(compile.stdout)}; stderr: ${cap(compile.stderr)}"
                )
                else -> {
                    val runResult = run(
                        java,
                        listOf("-XX:TieredStopAtLevel=1", "-XX:+UseSerialGC", "-cp", classes.absolutePath, "Hello"),
                        home,
                        RUN_TIMEOUT_SECONDS,
                    )
                    when {
                        runResult.timedOut -> report.problems.add(
                            "java (${java.absolutePath}) excedeu ${RUN_TIMEOUT_SECONDS}s executando a classe compilada."
                        )
                        runResult.startError != null -> report.problems.add(
                            "java (${java.absolutePath}) não iniciou o autoteste: ${runResult.startError}"
                        )
                        runResult.exitCode != 0 || !runResult.stdout.contains("HORIZON_JDK_SELFTEST_OK") ->
                            report.problems.add(
                                "java (${java.absolutePath}) não executou a classe compilada pelo javac " +
                                    "(código ${runResult.exitCode}). stdout: ${cap(runResult.stdout)}; " +
                                    "stderr: ${cap(runResult.stderr)}"
                            )
                    }
                }
            }
        } finally {
            selftest.deleteRecursively()
        }
    }

    /**
     * Replaces a `bin/javac` that is actually the java launcher with a POSIX sh
     * shim that runs the compiler through the working java launcher. The shim
     * resolves its own home from `$0`, so it survives the staging rename, and it
     * translates the `-J` options horizon-prepare passes (a plain `java` launcher
     * does not understand `-J`). It is only accepted after the full validation
     * (version + compile-and-run self test) passes through it.
     */
    private fun writeJavacShim(javac: File) {
        val script = listOf(
            "#!/system/bin/sh",
            "# Escrito pelo instalador do Horizon: o pacote publicado trouxe o launcher do",
            "# java no lugar do javac. Este shim executa o compilador pelo proprio java",
            "# (java -m jdk.compiler/com.sun.tools.javac.Main) e converte as opcoes -J.",
            "case \$0 in */*) ;; *) echo 'javac: caminho do shim invalido' >&2; exit 1;; esac",
            "bin=\${0%/*}",
            "jopts=\"\"",
            "for a in \"\$@\"; do",
            "  case \$a in",
            "    -J*) jopts=\"\$jopts \${a#-J}\" ;;",
            "  esac",
            "done",
            "first=1",
            "for a in \"\$@\"; do",
            "  case \$a in",
            "    -J*) ;;",
            "    *) if [ \"\$first\" -eq 1 ]; then set -- \"\$a\"; first=0; else set -- \"\$@\" \"\$a\"; fi ;;",
            "  esac",
            "done",
            "[ \"\$first\" -eq 1 ] && set --",
            "exec \"\$bin/java\" \$jopts -m jdk.compiler/com.sun.tools.javac.Main \"\$@\"",
            "",
        ).joinToString("\n")
        javac.delete()
        javac.writeText(script)
        javac.setReadable(true, true)
        javac.setExecutable(true, true)
    }

    private fun versionMatches(result: RunResult, regex: Regex): Boolean {
        if (result.timedOut || result.startError != null || result.exitCode != 0) return false
        return regex.containsMatchIn(result.stdout + "\n" + result.stderr)
    }

    private fun describeAttempts(tool: String, attempts: List<Pair<String, RunResult>>): String {
        return attempts.joinToString(" | ") { (flag, result) ->
            val status = when {
                result.timedOut -> "tempo esgotado"
                result.startError != null -> "não iniciou (${result.startError})"
                else -> "código ${result.exitCode}"
            }
            "[$tool $flag] $status; stdout: ${cap(result.stdout)}; stderr: ${cap(result.stderr)}"
        }
    }

    private fun cap(text: String): String {
        val trimmed = text.trim()
        return when {
            trimmed.isEmpty() -> "(vazia)"
            trimmed.length <= 400 -> trimmed
            else -> trimmed.take(400) + "..."
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
        private const val VERSION_TIMEOUT_SECONDS = 20L
        private const val COMPILE_TIMEOUT_SECONDS = 120L
        private const val RUN_TIMEOUT_SECONDS = 60L
        private val FLAGS = listOf("-version", "--version")
        private val ALLOWED_HOSTS = setOf("github.com", "release-assets.githubusercontent.com")
    }
}
