#!/usr/bin/env bash
# Somente a prova GPUI isolada. Não constrói o launcher nem o jogo.
set -euo pipefail
cd "$(dirname "$0")"
: "${ANDROID_HOME:?Android SDK ausente}"
: "${ANDROID_NDK_HOME:?Android NDK ausente}"
: "${GRADLE_CMD:?Gradle 8.14.3 ausente}"
TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/aarch64-linux-android30-clang"
export CC_aarch64_linux_android="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
export AR_aarch64_linux_android="$TOOLCHAIN/llvm-ar"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
test -x "$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
test -x "$TOOLCHAIN/llvm-nm"
test -x "$TOOLCHAIN/llvm-readelf"
test -x "$GRADLE_CMD"
# O lock é gerado nesta execução e guardado junto do APK para auditoria.
# Dependências transitivas podem variar entre execuções até o lock ser versionado.
cargo +1.99.0 generate-lockfile
export GPUI_ANDROID_SRC="$(cargo +1.99.0 metadata --locked --format-version 1 --filter-platform aarch64-linux-android | python3 -c 'import json,sys,pathlib; d=json.load(sys.stdin); p=[x for x in d["packages"] if x["name"]=="gpui_android" and (x.get("source") or "").endswith("#7f1bc62e6103c5bc02fc4757921d3f5cd132fc5a")]; assert len(p)==1, "Backend GPUI fixado não encontrado"; print(pathlib.Path(p[0]["manifest_path"]).parent / "android/src/main/kotlin")')"
test -f "$GPUI_ANDROID_SRC/org/holool/gpui/GpuiActivity.kt"
cargo +1.99.0 rustc --locked --lib --target aarch64-linux-android -- -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384
LIB="${CARGO_TARGET_DIR:-target}/aarch64-linux-android/debug/libhorizon_gpui_prova.so"
test -f "$LIB"
"$TOOLCHAIN/llvm-nm" -D --defined-only "$LIB" > "${RUNNER_TEMP:-/tmp}/gpui-prova-symbols.txt"
python3 check-apk-ci.py jni "$GPUI_ANDROID_SRC/org/holool/gpui/GpuiActivity.kt" "${RUNNER_TEMP:-/tmp}/gpui-prova-symbols.txt"
python3 check-elf-16k.py "$TOOLCHAIN/llvm-readelf" "$LIB"
LIB_DIR=host-android/app/src/main/jniLibs/arm64-v8a
mkdir -p "$LIB_DIR"
cp "$LIB" "$LIB_DIR/libhorizon_gpui_prova.so"
"$GRADLE_CMD" -p host-android --no-daemon --max-workers=2 -Pkotlin.compiler.execution.strategy=in-process assembleDebug
APK=host-android/app/build/outputs/apk/debug/app-debug.apk
BUILD_TOOLS="$ANDROID_HOME/build-tools/36.0.0"
test -f "$APK"
python3 check-apk-ci.py apk "$APK" "$LIB" "$TOOLCHAIN/llvm-readelf" check-elf-16k.py
"$BUILD_TOOLS/zipalign" -c -P 16 4 "$APK"
"$BUILD_TOOLS/apksigner" verify "$APK"
"$BUILD_TOOLS/aapt" dump badging "$APK" | python3 -c 'import sys; s=sys.stdin.read(); expected="package: name=\x27br.com.minehorizon.gpuiprova\x27"; assert expected in s, "ID da prova diferente do esperado"'
sha256sum "$APK" > host-android/app/build/outputs/apk/debug/SHA256SUMS.txt
printf '%s\n' 'APK de teste verificado estaticamente. Não é o launcher nem o jogo; execução em aparelho não verificada.'
