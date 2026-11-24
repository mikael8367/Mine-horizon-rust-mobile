# Inicialização Android do launcher original: análise e próximos bloqueios

Esta análise não implementa um launcher Android nem gera APK. Não foi comprovada compilação ou execução do launcher completo no Android. A UI desktop existente permanece inalterada.

## Caminho existente

A entrada atual `launcher/src/main.rs` chama `startup::run` em `launcher/src/startup.rs`; a raiz visual original é `Shell` em `launcher/src/window_root.rs`, que constrói o `Launcher`. Não substituí-la por uma interface alternativa.

O launcher e scenery usam `gpui-fast` 0.1.15; a prova Android `android/prova-gpui` usa o backend `holool/gpui_android` fixado e o `gpui` 0.2.2 do fork Holool. São identidades de crate distintas: seus tipos não são intercambiáveis. `gpui-fast-platform` 0.1.15 não seleciona uma plataforma Android. A prova isolada passou em `cargo check` para Android, mas isso não comprova a compatibilidade do launcher.

## Integração ainda necessária

- Harmonizar a identidade e as APIs de GPUI entre backend, launcher e scenery e compilar os módulos reais juntos. Preservar dependências e inicialização desktop.
- Adaptar a inicialização: o backend Android fornece Activity e entrada JNI pelo `android_main!`; não executar um segundo `Application::run`. O fluxo desktop também executa CLI e encerramento do processo, incompatíveis com uma Activity sem adaptação.
- Abrir o `Shell` real e seus assets no ciclo Android, observando que o backend experimental documenta uma única Activity/janela e não garante os serviços desktop. Verificar superfícies, suspensão, retomada, rotação, toque, teclado virtual e diretórios privados no aparelho.
- Ajustar dimensões e navegação: a janela desktop possui tamanho mínimo de 960×600; o módulo de geometria móvel ainda não está ligado à interface.
- O catálogo de releases ainda não contempla Android e o início do jogo usa subprocesso desktop. São necessários um runtime e ligação Android efetivos; abrir a interface não significa executar o jogo.
- O armazenamento atual da conta no Android recorre à memória; persistência segura de login requer implementação e validação específicas.

## Evidência e limites

Em 24/11/2026 foram feitas verificações estáticas de entrada, dependências, catálogo e armazenamento. Elas não equivalem a compilar ou testar em dispositivo. Um host de prova separado foi preparado localmente, mas a compilação Kotlin e linkedição nativa não terminaram com resultado verificável após perda da máquina temporária. Ele não está incluído como launcher funcional.

O utilitário `check-elf-16k.py` e seus testes verificam propriedades de alinhamento dos segmentos ELF antes do empacotamento, sem criar APK. Isso não valida APK, Android ou inicialização de jogo. Teste: `python3 -m unittest discover -s tests -v` a partir de `android/prova-gpui`.
