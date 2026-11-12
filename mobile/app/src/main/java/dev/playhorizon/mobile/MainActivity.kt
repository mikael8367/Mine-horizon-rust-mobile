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
import android.widget.EditText
import android.text.InputType
import dev.playhorizon.mobile.account.ElyByAccountStore
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.RadioButton
import android.widget.RadioGroup
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
    private var playerNameInput: EditText? = null
    private var downloadTask: Future<*>? = null
    private var workerBusy = false
    private var nativeEngineOpened = false
    private var selectedAccountMode = "offline"

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
        enterImmersiveLandscape()
        selectedAccountMode = getSharedPreferences("horizon_launcher", MODE_PRIVATE).getString("account_mode", "offline") ?: "offline"
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
            val statusFile = File(filesDir, "horizon-startup-status.txt")
            val lines = runCatching { statusFile.readLines(Charsets.UTF_8) }.getOrDefault(emptyList())
            val stage = lines.firstOrNull().orEmpty()
            val detail = lines.drop(1).joinToString(" ").trim()
            statusText?.text = when (stage) {
                "first_frame_presented" ->
                    "SUCESSO: a janela do Horizon abriu e apresentou a primeira tela.\\n" +
                        "A inicialização gráfica foi confirmada; ainda falta testar o gameplay no aparelho.\\n$detail"
                "boot_renderer_ready" ->
                    "A janela abriu e o renderer foi inicializado, mas a primeira tela ainda não foi confirmada.\\n$detail"
                "window_created" ->
                    "A janela nativa foi criada. O renderer ainda não confirmou a inicialização.\\n$detail"
                "window_create_failed" -> "Falha ao criar a janela do jogo: $detail"
                "renderer_boot_failed" -> "Falha ao inicializar a renderização: $detail"
                "resource_loading_failed" -> "Falha ao carregar os recursos do jogo: $detail"
                "event_loop_failed" -> "Falha no loop de eventos Android: $detail"
                "preflight_failed" -> "Os recursos não passaram na verificação nativa: $detail"
                "native_preflight_ok", "engine_starting", "event_loop_ready" ->
                    "O motor foi iniciado, mas ainda não confirmou a criação da janela.\\n$detail"
                "native_entry_started" -> "NativeActivity iniciou, mas o motor não avançou para a validação dos recursos."
                else -> "O motor nativo foi fechado sem registrar a primeira tela. Confira o log de inicialização."
            }
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

        content.addView(TextView(this).apply {
            text = "CONTA OFFLINE"
            textSize = 17f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(0xFFE8F5E9.toInt())
            setPadding(0, dp(8), 0, dp(6))
        })
        content.addView(TextView(this).apply {
            text = "Perfil local para jogar mundos singleplayer e servidores que aceitam modo offline. Não autentica em servidores online."
            textSize = 13f
            setTextColor(0xFFB8C9B8.toInt())
        })
        val playerName = EditText(this).apply {
            hint = "Nome do jogador (3–16 caracteres)"
            singleLine = true
            setTextColor(0xFFFFFFFF.toInt())
            setHintTextColor(0xFF9EAA9E.toInt())
            setText(loadOfflinePlayerName())
        }
        playerNameInput = playerName
        content.addView(playerName)
        content.addView(Button(this).apply {
            text = "Salvar perfil offline"
            setOnClickListener {
                val name = playerNameInput?.text?.toString()?.trim().orEmpty()
                if (!name.matches(Regex("[A-Za-z0-9_]{3,16}"))) {
                    statusText?.text = "Nome inválido. Use de 3 a 16 letras, números ou _."
                    return@setOnClickListener
                }
                try {
                    val target = File(filesDir, "horizon-player-name.txt")
                    val temp = File(filesDir, "horizon-player-name.txt.part")
                    temp.writeText(name, Charsets.UTF_8)
                    if (target.exists() && !target.delete()) error("Não foi possível substituir o perfil anterior")
                    if (!temp.renameTo(target)) error("Não foi possível salvar o perfil")
                    statusText?.text = "Perfil offline salvo: $name. O motor usará esse nome na próxima inicialização."
                } catch (error: Throwable) {
                    statusText?.text = "Falha ao salvar perfil: ${error.message ?: error.javaClass.simpleName}"
                }
            }
        })

        content.addView(TextView(this).apply {
            text = "CONTA ELY.BY"
            textSize = 17f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(0xFFE8F5E9.toInt())
            setPadding(0, dp(16), 0, dp(6))
        })
        content.addView(TextView(this).apply {
            text = "Entre com sua conta Ely.by. A senha é enviada somente por HTTPS e não é salva. O token fica criptografado pelo Android Keystore."
            textSize = 13f
            setTextColor(0xFFB8C9B8.toInt())
        })
        val elyUsername = EditText(this).apply {
            hint = "Usuário ou e-mail Ely.by"
            singleLine = true
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
            setTextColor(0xFFFFFFFF.toInt())
            setHintTextColor(0xFF9EAA9E.toInt())
        }
        val elyPassword = EditText(this).apply {
            hint = "Senha Ely.by"
            singleLine = true
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            setTextColor(0xFFFFFFFF.toInt())
            setHintTextColor(0xFF9EAA9E.toInt())
        }
        val elyTwoFactor = EditText(this).apply {
            hint = "Código 2FA (opcional)"
            singleLine = true
            inputType = InputType.TYPE_CLASS_NUMBER
            setTextColor(0xFFFFFFFF.toInt())
            setHintTextColor(0xFF9EAA9E.toInt())
        }
        content.addView(elyUsername)
        content.addView(elyPassword)
        content.addView(elyTwoFactor)
        val elyStore = ElyByAccountStore(filesDir)
        var elyDisconnectAction: Button? = null
        val elyAction = Button(this).apply {
            text = if (elyStore.hasAccount()) "Conectar novamente / atualizar conta Ely.by" else "Entrar com Ely.by"
            setOnClickListener {
                if (workerBusy) return@setOnClickListener
                val username = elyUsername.text?.toString()?.trim().orEmpty()
                val password = elyPassword.text?.toString()?.toCharArray() ?: CharArray(0)
                val twoFactorCode = elyTwoFactor.text?.toString()?.trim().orEmpty().takeIf { it.isNotEmpty() }
                if (username.isBlank() || password.isEmpty()) {
                    statusText?.text = "Informe o usuário e a senha Ely.by."
                    password.fill('\u0000')
                    return@setOnClickListener
                }
                isEnabled = false
                text = "Conectando à Ely.by..."
                statusText?.text = "Autenticando com Ely.by por HTTPS..."
                workerBusy = true
                downloadTask = worker.submit {
                    try {
                        val profile = elyStore.signIn(username, password, twoFactorCode)
                        runOnUiThread {
                            elyPassword.text?.clear()
                            elyTwoFactor.text?.clear()
                            statusText?.text = "Conta Ely.by conectada: ${profile.name}. Selecione Ely.by em 'Usar esta conta ao iniciar' para jogar com ela."
                            elyAction.text = "Conta Ely.by conectada: ${profile.name}"
                            elyDisconnectAction?.isEnabled = true
                        }
                    } catch (error: Throwable) {
                        runOnUiThread {
                            elyPassword.text?.clear()
                            elyTwoFactor.text?.clear()
                            statusText?.text = "Falha no login Ely.by: ${error.message ?: error.javaClass.simpleName}"
                            elyAction.text = "Tentar login Ely.by novamente"
                        }
                    } finally {
                        runOnUiThread {
                            isEnabled = true
                            workerBusy = false
                            downloadTask = null
                        }
                    }
                }
            }
        }
        content.addView(elyAction)
        content.addView(TextView(this).apply {
            text = "USAR ESTA CONTA AO INICIAR"
            textSize = 14f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(0xFFE8F5E9.toInt())
            setPadding(0, dp(10), 0, dp(4))
        })
        val accountModes = RadioGroup(this).apply {
            orientation = RadioGroup.HORIZONTAL
        }
        val offlineOption = RadioButton(this).apply {
            id = View.generateViewId()
            text = "Offline"
            setTextColor(0xFFE8F5E9.toInt())
        }
        val elyOption = RadioButton(this).apply {
            id = View.generateViewId()
            text = "Ely.by"
            setTextColor(0xFFE8F5E9.toInt())
        }
        accountModes.addView(offlineOption)
        accountModes.addView(elyOption)
        accountModes.check(if (selectedAccountMode == "elyby") elyOption.id else offlineOption.id)
        accountModes.setOnCheckedChangeListener { _, checkedId ->
            selectedAccountMode = if (checkedId == elyOption.id) "elyby" else "offline"
            getSharedPreferences("horizon_launcher", MODE_PRIVATE).edit()
                .putString("account_mode", selectedAccountMode).apply()
        }
        content.addView(accountModes)
        val disconnectButton = Button(this).apply {
            text = "Desconectar conta Ely.by"
            isEnabled = elyStore.hasAccount()
            setOnClickListener {
                if (workerBusy) return@setOnClickListener
                try {
                    elyStore.signOut()
                    selectedAccountMode = "offline"
                    getSharedPreferences("horizon_launcher", MODE_PRIVATE).edit()
                        .putString("account_mode", "offline").apply()
                    accountModes.check(offlineOption.id)
                    elyPassword.text?.clear()
                    elyTwoFactor.text?.clear()
                    elyUsername.text?.clear()
                    elyAction.text = "Entrar com Ely.by"
                    elyDisconnectAction?.isEnabled = false
                    statusText?.text = "Conta Ely.by desconectada. Token, sessão temporária e chave local foram removidos."
                } catch (error: Throwable) {
                    statusText?.text = "Falha ao desconectar Ely.by: ${error.message ?: error.javaClass.simpleName}"
                }
            }
        }
        elyDisconnectAction = disconnectButton
        content.addView(disconnectButton)

        val downloadAction = Button(this).apply {
            text = "Baixar arquivos do Minecraft"
            setOnClickListener { startMinecraftDownload() }
        }
        downloadButton = downloadAction
        content.addView(downloadAction)

        val prepareAction = Button(this).apply {
            text = "Baixar e preparar recursos vanilla"
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
                if (!preparedManifest.isFile || preparedManifest.length() <= 2L ||
                    !preparedStamp.isFile || preparedStamp.length() <= 2L
                ) {
                    statusText?.text = "Os recursos vanilla ainda não foram preparados ou algum manifesto está vazio. Use Baixar e preparar recursos vanilla."
                    return@setOnClickListener
                }
                statusText?.text = "Recursos vanilla validados. Abrindo o motor Rust ARM64..."
                try {
                    val accountStore = ElyByAccountStore(filesDir)
                    if (selectedAccountMode == "elyby") {
                        if (!accountStore.hasAccount()) error("Conecte uma conta Ely.by antes de selecioná-la.")
                        accountStore.createLaunchSessionFile()
                    } else {
                        File(filesDir, "horizon-session.json").delete()
                    }
                    File(filesDir, "horizon-startup-status.txt").delete()
                    File(filesDir, "horizon-startup-status.txt.part").delete()
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
        prepareButton?.text = "Baixando arquivos oficiais..."
        cancelButton?.text = "Cancelar operação"
        cancelButton?.visibility = View.VISIBLE
        progressBar?.progress = 0
        statusText?.text = "Etapa 1/2: consultando o manifesto oficial da Mojang..."

        downloadTask = worker.submit {
            try {
                // Re-resolve the official 26.3 plan on every run. Existing files
                // are reused only after exact-size and SHA-1 verification.
                val plan = MojangDownloadPlan().resolve()
                val gameDirectory = File(filesDir, "minecraft/26.3")
                val installed = MinecraftDataInstaller().install(
                    plan = plan,
                    gameDirectory = gameDirectory,
                    isCancelled = { Thread.currentThread().isInterrupted },
                ) { progress ->
                    val fraction = if (progress.totalBytes > 0L) {
                        progress.completedBytes.toDouble() / progress.totalBytes
                    } else 0.0
                    val currentMb = progress.fileProgress.downloaded / 1_048_576.0
                    val totalMb = progress.fileProgress.total.takeIf { it > 0L }?.div(1_048_576.0)
                    val fileText = if (totalMb != null) {
                        "${formatMb(currentMb)} / ${formatMb(totalMb)} MB"
                    } else {
                        "${formatMb(currentMb)} MB"
                    }
                    runOnUiThread {
                        progressBar?.progress = (fraction.coerceIn(0.0, 1.0) * 450).toInt()
                        statusText?.text =
                            "Etapa 1/2: baixando e verificando ${progress.currentPath}\n$fileText\n" +
                                "${formatMb(progress.completedBytes / 1_048_576.0)} / " +
                                "${formatMb(progress.totalBytes / 1_048_576.0)} MB"
                    }
                }
                if (Thread.currentThread().isInterrupted) {
                    throw CancellationException("Download dos arquivos vanilla cancelado")
                }
                runOnUiThread {
                    progressBar?.progress = 450
                    statusText?.text =
                        "Etapa 1/2 concluída: arquivos oficiais do Minecraft 26.3 verificados.\n" +
                            "Etapa 2/2: instalando o OpenJDK 25 ARM64 e preparando os recursos do Horizon..."
                    prepareButton?.text = "Preparando recursos do Horizon..."
                }

                val destination = VanillaResourcePreparer(this).prepare(
                    isCancelled = { Thread.currentThread().isInterrupted },
                ) { progress ->
                    runOnUiThread {
                        progress.fraction?.let {
                            // 45% is reserved for Mojang downloads; 55% for JDK
                            // installation and Horizon preparation.
                            progressBar?.progress =
                                (450 + it.coerceIn(0.0, 1.0) * 550).toInt()
                        }
                        statusText?.text = "Etapa 2/2: ${progress.message}"
                    }
                }
                runOnUiThread {
                    progressBar?.progress = 1000
                    statusText?.text =
                        "DOWNLOAD E PREPARAÇÃO CONCLUÍDOS!\n" +
                            "Minecraft 26.3: arquivos oficiais verificados.\n" +
                            "Horizon: manifestos gerados e validação --check aprovada.\n" +
                            "Arquivos: ${installed.absolutePath}\nRecursos: ${destination.absolutePath}"
                    prepareButton?.text = "Recursos vanilla concluídos"
                }
            } catch (error: Throwable) {
                val cancelled = error is CancellationException || Thread.currentThread().isInterrupted
                runOnUiThread {
                    statusText?.text = if (cancelled) {
                        "Operação cancelada. Os arquivos já verificados são mantidos e os downloads incompletos podem ser retomados."
                    } else {
                        "Falha no download/preparação vanilla: " +
                            (error.message ?: error.javaClass.simpleName)
                    }
                    prepareButton?.text = "Tentar baixar e preparar novamente"
                }
            } finally {
                runOnUiThread {
                    workerBusy = false
                    downloadTask = null
                    cancelButton?.visibility = View.GONE
                    cancelButton?.text = "Cancelar download"
                    downloadButton?.isEnabled = true
                    prepareButton?.isEnabled = true
                }
            }
        }
    }


    private fun loadOfflinePlayerName(): String {
        return runCatching {
            File(filesDir, "horizon-player-name.txt").readText(Charsets.UTF_8).trim()
        }.getOrNull()?.takeIf { it.matches(Regex("[A-Za-z0-9_]{3,16}")) } ?: "Steve"
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
