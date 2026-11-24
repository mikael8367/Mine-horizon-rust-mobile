# Prova isolada GPUI Android

Este esqueleto Rust **não é a interface real do launcher**, não integra o launcher nem o jogo e não comprova a compatibilidade deles. A prova pretende abrir uma view mínima GPUI quando iniciada por um host Android adequado, mas isso ainda não foi verificado. A experiência desktop do projeto não é alterada: este diretório tem workspace Cargo próprio, e o código e as dependências GPUI ficam restritos ao alvo Android.

Nenhum APK foi gerado. A prova, sozinha, não é um app Android executável: é uma biblioteca `cdylib` com o ponto de entrada JNI do backend; requer o host Kotlin e configuração Android descritos pelo upstream. Não copiar ou executar o script completo de build upstream para esta validação, pois ele também gera um APK.

A prova também inclui diretamente o arquivo original `launcher/scenery/src/art.rs`, sem copiá-lo nem alterá-lo, e chama sua função `bake`: é uma primeira sondagem de tipos e API com código real do projeto. Isso não testa renderização da imagem, não usa a interface inteira e depende de checkout completo com esse arquivo.

## Origem e limites

- Backend [`holool/gpui_android` fixado em `7f1bc62e6103c5bc02fc4757921d3f5cd132fc5a`](https://github.com/holool/gpui_android/tree/7f1bc62e6103c5bc02fc4757921d3f5cd132fc5a).
- GPUI [`holool/gpui-fast` fixado em `c1d2067ce521826a138f7d9d4fe94299b83ff267`](https://github.com/holool/gpui-fast/tree/c1d2067ce521826a138f7d9d4fe94299b83ff267), o mesmo declarado pelo backend. A view usa chamadas do [exemplo upstream](https://github.com/holool/gpui_android/blob/7f1bc62e6103c5bc02fc4757921d3f5cd132fc5a/examples/minimal/src/lib.rs), e a abertura e macro seguem as [instruções de incorporação](https://github.com/holool/gpui_android/blob/7f1bc62e6103c5bc02fc4757921d3f5cd132fc5a/README.md#embed-a-rust-gpui-application).
- Backend inicial/experimental, com uma Activity e janela por aplicação; não é garantia de suporte para menus, janelas auxiliares, entradas, recursos, gráficos ou bibliotecas do launcher original. Mesmo que esta biblioteca compile, nada disso estará demonstrado.
- Sem `Cargo.lock` validado: as revisões Git diretas estão fixadas, mas dependências transitivas podem mudar. A verificação Android descrita abaixo passou, mas o build não é reproduzível e a linkedição ainda não foi validada.

## Validação sem APK

Em 24/11/2026, em ambiente remoto temporário, `cargo +1.99.0 check --lib --target aarch64-linux-android` concluiu para a biblioteca da prova usando Rust 1.99.0 e NDK 27.1.12297006. Isso não valida a linkedição, a execução, o launcher completo nem o jogo. Sem Cargo.lock validado, resultados futuros podem variar. Os requisitos usados no exemplo upstream incluem Rust 1.99.0, alvo `aarch64-linux-android`, NDK 27.1.12297006, API Android 30 e o LLVM do NDK correspondente ao computador. Defina `ANDROID_NDK_HOME` e `NDK_HOST` (por exemplo, `linux-x86_64`) de acordo com a instalação:

```sh
cd android/prova-gpui
rustup target add --toolchain 1.99.0 aarch64-linux-android
export NDK_BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$NDK_HOST/bin"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android30-clang"
export CC_aarch64_linux_android="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
export AR_aarch64_linux_android="$NDK_BIN/llvm-ar"
cargo +1.99.0 check --lib --target aarch64-linux-android
```

O `check` não gera APK e não valida linkedição nem execução. Se bem-sucedido, ele verificará a compatibilidade de compilação desse arquivo de arte com a revisão do GPUI usada na prova, não a do launcher inteiro. Para testar também a linkedição da biblioteca nativa, sem criar APK:

```sh
cargo +1.99.0 rustc --lib --target aarch64-linux-android -- \
  -C link-arg=-Wl,-z,max-page-size=16384 \
  -C link-arg=-Wl,-z,common-page-size=16384
```

Um `cargo +1.99.0 check --lib` no host pode verificar o manifesto e o lado não Android, mas não compila a view Android devido a `cfg` e não é prova de compatibilidade Android. Os comandos Cargo podem criar `Cargo.lock` e `target` **neste workspace** e baixar dependências.

Para qualquer futura execução, será necessário integrar o `android/src/main/kotlin` do backend, usar uma Activity derivada de `org.holool.gpui.GpuiActivity` que retorne `horizon_gpui_prova` em `nativeLibraryName()`, fornecer manifesto/configurações e posicionar a `.so` na ABI correta, conforme upstream. Esse host não está incluído nem foi testado aqui. A macro `android_main!` fornece a entrada JNI e o backend gerencia o ciclo da aplicação; não chamar `Application::run` adicionalmente. Execução em dispositivo, lifecycle, renderização e toque permanecem pendentes e qualquer empacotamento APK depende de solicitação separada.

## Resultado da verificação inicial (24/11/2026)

O manifesto foi analisado como TOML e o arquivo de arte foi confirmado na branch. O código original de `art.rs` passou em seis testes Rust no host: o teste existente de dimensões e cinco verificações adicionais de expansão, arredondamento, tamanho mínimo, transparência e canais/alpha pré-multiplicado. Para esse teste foi usado um substituto simples de `RenderImage` no lugar de GPUI. Portanto, o resultado valida apenas a lógica de arte nessas condições, não valida o GPUI verdadeiro, o backend Android ou a compilação do projeto. As cinco verificações adicionais foram executadas temporariamente e não foram adicionadas à suíte do repositório.
