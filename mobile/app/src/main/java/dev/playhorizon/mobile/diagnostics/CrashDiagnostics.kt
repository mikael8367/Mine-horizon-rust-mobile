package dev.playhorizon.mobile.diagnostics

import android.content.Context
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Diagnóstico persistente de inicialização e falhas do Horizon Rust Mobile.
 *
 * Tudo é gravado no armazenamento privado do aplicativo
 * (`filesDir/horizon-diagnostics/`), visível sem root pelo próprio launcher
 * (a tela inicial mostra o último crash registrado) e copiável pelo usuário.
 *
 * Dois arquivos:
 * - `launch-log.txt`: marcos da inicialização (processo, onCreate, UI,
 *   contas, checagem da biblioteca nativa, pedido de abertura do motor).
 * - `crash-log.txt`: stack traces completos de toda exceção não tratada.
 *
 * Nenhum campo registra senhas, tokens ou credenciais: só o texto passado
 * explicitamente por quem chama. Exceções nunca são engolidas: o handler
 * encadeia para o handler anterior do Android depois de gravar o log.
 */
object CrashDiagnostics {
    private const val DIR = "horizon-diagnostics"
    private const val LAUNCH_FILE = "launch-log.txt"
    private const val CRASH_FILE = "crash-log.txt"
    private const val MAX_LOG_BYTES = 512L * 1024L
    private const val MAX_CRASH_BYTES = 256L * 1024L

    private val timestamp = SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS", Locale.US)

    @Volatile
    private var previousHandler: Thread.UncaughtExceptionHandler? = null

    /** Instala o capturador de exceções. Chamar antes de qualquer outro código da UI. */
    @Synchronized
    fun install(context: Context) {
        if (previousHandler != null) return
        val app = context.applicationContext
        previousHandler = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            runCatching { appendCrash(app, thread, throwable) }
            // Nunca substitui o comportamento do Android: o processo ainda
            // precisa morrer como morreria (diálogo, reinício do Activity).
            previousHandler?.uncaughtException(thread, throwable)
        }
    }

    /** Registra um marco da inicialização com horário. */
    fun stage(context: Context, stage: String, detail: String = "") {
        val app = context.applicationContext
        val line = "[${timestamp.format(Date())}] $stage" +
            (if (detail.isBlank()) "" else " | $detail") + "\n"
        appendCapped(File(File(app.filesDir, DIR), LAUNCH_FILE), line, MAX_LOG_BYTES)
    }

    /** Resumo da última exceção não tratada, para exibir na tela do launcher. */
    fun lastCrashSummary(context: Context): String? {
        val file = crashFile(context)
        if (!file.isFile) return null
        val text = runCatching { file.readText(Charsets.UTF_8) }.getOrNull() ?: return null
        val last = text.split(CRASH_SEPARATOR)
            .map { it.trim() }
            .lastOrNull { it.isNotEmpty() } ?: return null
        // Primeiras linhas bastam para identificar a causa; o arquivo completo fica no disco.
        return last.lines().take(12).joinToString("\n")
    }

    fun crashFile(context: Context): File = File(File(context.applicationContext.filesDir, DIR), CRASH_FILE)

    fun launchFile(context: Context): File = File(File(context.applicationContext.filesDir, DIR), LAUNCH_FILE)

    private fun appendCrash(context: Context, thread: Thread, throwable: Throwable) {
        val body = buildString {
            append(CRASH_SEPARATOR.trimStart())
            append("[${timestamp.format(Date())}] FATAL EXCEPTION na thread \"${thread.name}\" (id ${thread.id})\n")
            append(throwable.javaClass.name).append(": ").append(throwable.message ?: "").append("\n")
            append(throwable.stackTraceToString())
            var cause = throwable.cause
            var depth = 0
            while (cause != null && depth < 8) {
                append("Causada por: ").append(cause.javaClass.name).append(": ")
                    .append(cause.message ?: "").append("\n")
                append(cause.stackTraceToString())
                cause = cause.cause
                depth++
            }
        }
        appendCapped(crashFile(context), body, MAX_CRASH_BYTES)
    }

    private fun appendCapped(file: File, text: String, maxBytes: Long) {
        runCatching {
            file.parentFile?.let { parent ->
                if (!parent.isDirectory && !parent.mkdirs()) return
            }
            // Mantém o final do arquivo (o histórico mais recente) ao estourar o limite.
            if (file.isFile && file.length() + text.length > maxBytes) {
                val keep = file.readText(Charsets.UTF_8)
                    .takeLast((maxBytes / 2).toInt())
                    .substringAfter('\n', "")
                val temp = File(file.parentFile, file.name + ".part")
                temp.writeText(keep, Charsets.UTF_8)
                if (!temp.renameTo(file)) temp.delete()
            }
            file.appendText(text, Charsets.UTF_8)
        }
    }

    private const val CRASH_SEPARATOR = "================ CRASH ================\n"
}
