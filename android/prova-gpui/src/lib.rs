// Este módulo é o próprio arquivo do launcher, sem cópia ou alteração.
// Ele usa GPUI + image e fornece uma sondagem real de compatibilidade de API.
#[cfg(target_os = "android")]
#[path = "../../../launcher/scenery/src/art.rs"]
mod launcher_art;

// Prova isolada do backend, não é a interface do launcher.
#[cfg(target_os = "android")]
mod mobile {
    use gpui::*;

    struct Prova;

    impl Render for Prova {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(rgb(0x101827))
                .text_color(rgb(0xffffff))
                .p_8()
                .child("Prova GPUI Android — não é o launcher")
        }
    }

    pub fn launch(cx: &mut App) {
        // Executa o código original de criação de imagem. Ainda não testa sua
        // renderização nem comprova compatibilidade com o launcher completo.
        let _imagem_do_launcher = crate::launcher_art::bake(&[[0x11, 0x22, 0x33, 255]], 1, 1, 1.0, 1.0);
        cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Prova))
            .expect("abrir janela GPUI Android");
    }
}

#[cfg(target_os = "android")]
gpui_android::android_main!(mobile::launch);
