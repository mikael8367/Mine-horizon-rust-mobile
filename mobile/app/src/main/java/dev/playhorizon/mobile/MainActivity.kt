package dev.playhorizon.mobile

import android.app.Activity
import android.content.pm.ActivityInfo
import android.content.Intent
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import dev.playhorizon.mobile.download.MinecraftDataInstaller
import dev.playhorizon.mobile.download.MojangDownloadPlan
import dev.playhorizon.mobile.download.VanillaResourcePreparer
import java.io.File
import java.util.Locale
import java.util.concurrent.Executors
import java.util.concurrent.Future
import java.util.concurrent.CancellationException

/**
 * Minimal Android download screen. It downloads and verifies the official
 * Minecraft inputs; it does not claim that the native Rust engine is playable.
 */
class MainActivity : Activity() {
    private val worker = Executors.newSingleThreadExecutor()
    private var downloadButton: Button? = null
    private var progressBar: ProgressBar? = null
    private var statusText: TextView? = null
    private var cancelButton: Button? = null
    private var prepareButton: Button? = null
    private var downloadTask: Future<*>? = null
    private var workerBusy = false
    private var nativeEngineOpened = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
        enterImmersiveLandscape()
        buildDownloadScreen()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) enterImmersiveLandscape()
    }

    override fun onResume() {
        super.onResume()
        if (nativeEngineOpened) {
            nativeEngineOpened = false
            statusText?.text = "Você voltou ao launcher. O motor nativo foi fechado."
        }
    }

    override fun onDestroy() {
        worker.shutdownNow() // any .part file is retained for a later resume
        super.onDestroy()
    }

    private fun buildDownloadScreen() {
        val density = resources.displayMetrics.density
        fun dp(value: Int) = (value * density).toInt()
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(28), dp(20), dp(28), dp(20))
            setBackgroundColor(0xFF101810.toInt())
        }
        val scroll = ScrollView(this)
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        content.addView(TextView(this).apply {
            text = "HORIZON RUST MOBILE"
            textSize = 24f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(0xFFE8F5E9.toInt())
        })
        content.addView(TextView(this).apply {
            text = "Minecraft Java Edition 26.3 — arquivos oficiais"
            textSize = 16f
            setTextColor(0xFFB8C9B8.toInt())
            setPadding(0, dp(8), 0, dp(12))
        })

        val downloadAction = Button(this).apply {
            text = "Baixar arquivos do Minecraft"
            setOnClickListener { startMinecraftDownload() }
        }
        downloadButton = downloadAction
        content.addView(downloadAction)

        val prepareAction = Button(this).apply {
            text = "Preparar recursos vanilla"
            setOnClickListener { startVanillaPreparation() }
        }
        prepareButton = prepareAction
        content.addView(prepareAction)

        content.addView(Button(this).apply {
            text = "Abrir motor Rust em tela cheia"
            setOnClickListener {
                val nativeLibrary = File(applicationInfo.nativeLibraryDir, "libhorizon_android.so")
                if (!nativeLibrary.isFile) {
                    statusText?.text = "Biblioteca ARM64 ainda não foi incluída. Aguarde a compilação do CI."
                    return@setOnClickListener
                }
                val preparedDir = File(filesDir, ".local/horizon-client/26.3")
                val preparedManifest = File(preparedDir, "horizon-resources.json")
                val preparedStamp = File(preparedDir, "horizon-prepare.json")
                if (!preparedManifest.isFile || !preparedStamp.isFile) {
                    statusText?.text = "Os recursos vanilla ainda não foram preparados e validados. Use o botão Preparar recursos vanilla."
                    return@setOnClickListener
                }
                statusText?.text = "Recursos vanilla validados. Abrindo o motor Rust ARM64..."
                try {
                    nativeEngineOpened = true
                    startActivity(Intent(this@MainActivity, android.app.NativeActivity::class.java))
                } catch (error: Throwable) {
                    nativeEngineOpened = false
                    statusText?.text = "Não foi possível abrir o motor nativo: " + (error.message ?: error.javaClass.simpleName)
                }
            }
        })

        val cancelAction = Button(this).apply {
            text = "Cancelar download"
            visibility = View.GONE
            setOnClickListener {
                downloadTask?.cancel(true)
                statusText?.text = "Cancelando download... Os arquivos temporários válidos serão mantidos para retomar."
            }
        }
        cancelButton = cancelAction
        content.addView(cancelAction)

        val progress = ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal).apply {
            max = 1000
            progress = 0
        }
        progressBar = progress
        content.addView(progress, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT, dp(12)
        ).apply { topMargin = dp(12) })

        val status = TextView(this).apply {
            text = "Pronto para verificar os arquivos oficiais da versão 26.3."
            textSize = 14f
            setTextColor(0xFFE0E0E0.toInt())
            setPadding(0, dp(10), 0, dp(16))
        }
        statusText = status
        content.addView(status)

        content.addView(TextView(this).apply {
            text = "RUNTIME NATIVO ARM64"
            textSize = 17f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(0xFFE8F5E9.toInt())
        })
        content.addView(TextView(this).apply {
            text = "A preparação usa o OpenJDK 25 ARM64 e o preparador Rust do Horizon. Os arquivos oficiais são verificados antes da preparação. O jogo ainda precisa de autenticação/licença Java Edition e validação de renderização em aparelho real."
            textSize = 14f
            setTextColor(0xFFFFD180.toInt())
            setPadding(0, dp(8), 0, dp(12))
        })
        scroll.addView(content)
        root.addView(scroll, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f
        ))
        setContentView(root)
    }

    private fun startMinecraftDownload() {
        if (workerBusy) return
        workerBusy = true
        downloadButton?.isEnabled = false
        downloadButton?.text = "Preparando download..."
        progressBar?.progress = 0
        statusText?.text = "Consultando o manifesto oficial da Mojang..."

        cancelButton?.visibility = View.VISIBLE
        downloadTask = worker.submit {
            try {
                val plan = MojangDownloadPlan().resolve()
                val installer = MinecraftDataInstaller()
                val destination = File(filesDir, "minecraft/26.3")
                val installed = installer.install(
                    plan = plan,
                    gameDirectory = destination,
                    isCancelled = { Thread.currentThread().isInterrupted },
                ) { progress ->
                    runOnUiThread {
                        val fraction = if (progress.totalBytes > 0L) {
                            progress.completedBytes.toDouble() / progress.totalBytes
                        } else 0.0
                        progressBar?.progress = (fraction.coerceIn(0.0, 1.0) * 1000).toInt()
                        val currentMb = progress.fileProgress.downloaded / 1_048_576.0
                        val totalMb = progress.fileProgress.total.takeIf { it > 0L }?.div(1_048_576.0)
                        val fileText = if (totalMb != null) {
                            "${formatMb(currentMb)} / ${formatMb(totalMb)} MB"
                        } else {
                            "${formatMb(currentMb)} MB"
                        }
                        statusText?.text =
                            "Baixando e verificando: ${progress.currentPath}\n$fileText\n" +
                                "${formatMb(progress.completedBytes / 1_048_576.0)} / " +
                                "${formatMb(progress.totalBytes / 1_048_576.0)} MB"
                    }
                }
                runOnUiThread {
                    statusText?.text =
                        "Arquivos oficiais baixados e verificados.\nLocal: ${installed.absolutePath}\n" +
                            "Os arquivos estão prontos, mas ainda não são recursos preparados do Horizon. A preparação vanilla é uma etapa separada."
                    progressBar?.progress = 1000
                }
            } catch (error: Throwable) {
                val wasCancelled = error is CancellationException || Thread.currentThread().isInterrupted
                runOnUiThread {
                    statusText?.text = if (wasCancelled) {
                        "Download cancelado. Os arquivos temporários podem ser retomados."
                    } else {
                        "Download não concluído: ${error.message ?: error.javaClass.simpleName}"
                    }
                }
            } finally {
                runOnUiThread {
                    workerBusy = false
                    downloadTask = null
                    cancelButton?.visibility = View.GONE
                    downloadButton?.isEnabled = true
                    downloadButton?.text = "Baixar arquivos do Minecraft"
                }
            }
        }
    }

    private fun startVanillaPreparation() {
        if (workerBusy) return
        workerBusy = true
        downloadButton?.isEnabled = false
        prepareButton?.isEnabled = false
        prepareButton?.text = "Preparando recursos..."
        cancelButton?.visibility = View.VISIBLE
        progressBar?.progress = 0
        statusText?.text = "Iniciando preparação vanilla..."

        downloadTask = worker.submit {
            try {
                val destination = VanillaResourcePreparer(this).prepare(
                    isCancelled = { Thread.currentThread().isInterrupted },
                ) { progress ->
                    runOnUiThread {
                        progress.fraction?.let {
                            progressBar?.progress = (it.coerceIn(0.0, 1.0) * 1000).toInt()
                        }
                        statusText?.text = progress.message
                    }
                }
                runOnUiThread {
                    progressBar?.progress = 1000
                    statusText?.text = "Recursos vanilla preparados e validados!\nLocal: " + destination.absolutePath
                    prepareButton?.text = "Recursos vanilla preparados"
                }
            } catch (error: Throwable) {
                val cancelled = error is CancellationException || Thread.currentThread().isInterrupted
                runOnUiThread {
                    statusText?.text = if (cancelled) {
                        "Preparação cancelada. Execute novamente para continuar."
                    } else {
                        "Falha na preparação vanilla: " + (error.message ?: error.javaClass.simpleName)
                    }
                    prepareButton?.text = "Tentar preparar novamente"
                }
            } finally {
                runOnUiThread {
                    workerBusy = false
                    downloadTask = null
                    cancelButton?.visibility = View.GONE
                    downloadButton?.isEnabled = true
                    prepareButton?.isEnabled = true
                }
            }
        }
    }

    private fun formatMb(value: Double): String = String.format(Locale.ROOT, "%.1f", value)

    private fun enterImmersiveLandscape() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false)
            window.insetsController?.let { controller ->
                controller.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
                controller.systemBarsBehavior =
                    WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = (
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    or View.SYSTEM_UI_FLAG_FULLSCREEN
                    or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                )
        }
    }
}
