package br.com.minehorizon.gpuiprova

import org.holool.gpui.GpuiActivity

/** Host apenas para a prova GPUI: não inicia o launcher nem o jogo. */
class ProvaActivity : GpuiActivity() {
    override fun nativeLibraryName(): String = "horizon_gpui_prova"
}
