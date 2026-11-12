# Horizon Rust Mobile — Android module

This is the Android host module for the existing Horizon Rust game. It is not a
standalone replacement game or a Java Edition launcher.

## Minecraft game-data download

The landscape Android screen now has a real download action for the Horizon-pinned
Minecraft 26.3 inputs. It resolves Mojang's official version manifest, verifies
the pinned version JSON and asset-index SHA-1 values, and creates a download plan
for the version JSON, client/server jars, Maven libraries and every asset object.

Downloads use HTTPS allow-listed Mojang hosts, SHA-1 plus exact byte-size checks,
temporary `.part` files, HTTP range resume, cancellation checks and progress
callbacks. Existing files are re-hashed before being reused. Files are stored in
the app-private `files/minecraft/26.3` directory. No Mojang game files are stored
in this repository or in CI.

The download stage is separate from resource preparation. The launcher also
provides offline and Ely.by account modes and a native-engine launch action.
Offline mode supports local single-player and servers that accept offline
authentication; it does not bypass online-mode authentication. Microsoft
device-code sign-in and Java Edition entitlement validation remain optional
work for Microsoft-authenticated servers.

## Runtime and preparation

Horizon gameplay is Rust-native; it does not need a desktop Java VM to execute
the game. The Android ARM64 engine library (`libhorizon_android.so`) is now
cross-compiled by CI, staged for Gradle packaging, and loaded through Android
NativeActivity. This native library is the gameplay runtime; a separate desktop
JRE or Linux runtime archive must not be downloaded for Android.

The existing `horizon-prepare` pipeline invokes both `java` and `javac` 25.
The Android shell now includes a real preparation action: CI cross-compiles the
Rust `horizon-prepare` executable for ARM64 and bundles only Horizon's lock/source
files (never Mojang game files). A separate manual workflow,
`.github/workflows/android-jdk25.yml`, builds the full Android ARM64 OpenJDK 25
(`java` + `javac`) and publishes a SHA-256-manifested release asset. The launcher action **Baixar e preparar recursos vanilla** now runs the complete
two-stage pipeline: it resolves Mojang's official 26.3 metadata, downloads or
reuses the version JSON, client/server jars, Maven libraries, asset index and
all asset objects, and verifies every file by exact size and SHA-1. It then
downloads/verifies the Android ARM64 JDK release, safely extracts it into
app-private storage, runs the ARM64 `horizon-prepare` executable against those
game files, and requires both output manifests plus a successful
`horizon-prepare --check` before reporting completion. The standalone
**Baixar arquivos do Minecraft** action remains available. This is a preparation
toolchain; Rust gameplay remains native.

The JDK release workflow must be run once from GitHub Actions before the in-app
preparation button can download its runtime. Successful CI compilation does not
prove that Android permits execution of the packaged ARM64 preparer/JDK on every
device; the first on-device run remains required to validate this.

## Current implementation status

- Android app shell is landscape-first and uses immersive mode.
- The launcher UI remains in `MainActivity`; the native Rust engine opens as a separate fullscreen `NativeActivity` on top of it. Finishing the native activity (Android Back) returns to the launcher instead of replacing its screen.
- The launcher opens NativeActivity only after both Horizon preparation manifests exist and are non-empty. The “Baixar e preparar recursos vanilla” action downloads/verifies all official Minecraft 26.3 inputs, installs the Android Java 25 toolchain, executes the Rust preparer, and validates its output with `--check`.
- NativeActivity validates both prepared manifests, creates separate app-private folders for engine logs, game saves and player settings, and passes explicit `--home`, `--game-dir`, `--player-dir`, and `--resources` paths. The Rust startup reports event-loop creation, native window creation, renderer initialization, resource-loading errors, the first presented title frame, and successful handoff to the actual game client. The launcher displays the last confirmed milestone on return.
- The window and renderer are created by Horizon's existing winit/wgpu loading-screen path. Android-device validation is still required to confirm the PowerVR driver can create the surface and present frames.
- `MojangDownloadPlan.kt` resolves the pinned 26.3 version JSON, checks its
  accepted SHA-1, checks the pinned client/server jars and accepted asset index,
  then enumerates library artifacts and content-addressed asset objects.
- `MinecraftDataInstaller.kt` installs files sequentially into app-private
  storage and exposes total/file progress. Existing files are reused only after
  size and SHA-1 verification.
- `VerifiedDownloader.kt` restricts HTTPS hosts and redirects, resumes partial
  transfers, streams SHA-1/SHA-256 verification without loading large files into
  memory, checks exact file sizes, and finalizes verified files atomically.
- The download button is connected to this pipeline. It does not mark the game
  prepared. The preparation button is connected to the Android JDK installer and
  ARM64 `horizon-prepare` process; progress, cancellation, output validation and
  error messages are surfaced in the launcher.
- The Android shell workflow compiles `horizon-prepare` for ARM64 and stages
  `research-lock.json`, `exporters.rs`, and Horizon's Java parity sources into
  app assets. These are source inputs only; Minecraft jars/assets remain user
  downloads and are never embedded in CI.
- The Android JDK workflow is manual and publishes
  `horizon-jdk25-android-arm64.zip` plus `runtime.json` to the release tag
  `android-jdk25-arm64`. The app verifies the archive SHA-256 from that manifest
  and checks both `java -version` and `javac -version` before use.
- Android CI cross-builds the ARM64 native library, verifies its ELF64/AArch64 architecture, inspects its ELF `DT_NEEDED` entries, rejects unaccounted shared-library dependencies, and stages the engine plus any required `libc++_shared.so` from the matching NDK. It then checks that Gradle merges every ARM64 shared library byte-for-byte. These are the native runtime libraries the Rust engine actually links against; Android platform libraries (for example `libandroid.so`, `liblog.so`, and the GPU driver libraries) are supplied by Android and must not be copied from a desktop Linux distribution.
- The Minecraft download plan installs the pinned version JSON, client/server jars, Maven library artifacts, the asset index, and every content-addressed asset object. These Java/Maven libraries are preparation inputs and game data; they are not a Java gameplay runtime. Horizon gameplay is the Rust ARM64 native library, while the full OpenJDK 25 ARM64 archive is used only by `horizon-prepare`. Native Java desktop classifiers (Windows/macOS/Linux) are not valid Android runtime libraries and must not be loaded into the Android process.
- Gradle explicitly limits this app build to `arm64-v8a` and enables legacy JNI-library packaging so Android extracts `libhorizon_android.so` into `nativeLibraryDir`, matching the launcher's library-presence check and `NativeActivity` loading path.
- Added a `horizon-android` NativeActivity entry point and passed AndroidApp into winit's event-loop builder. CI builds the ARM64 `.so` and verifies Gradle packaging; actual surface creation and frame presentation still require a real-device test.
- The touch-control engine exists in `horizon-client-render/src/touch.rs`, but it is not yet connected to the game's Android touch events and HUD; on-screen joystick, jump, attack, and look controls remain incomplete.
- Android audio output is not yet wired into the Rust audio backend. Keyboard/controller support and real-device GPU/input/lifecycle validation remain to be tested.
- Ely.by direct authentication is implemented in the launcher/native session path but still needs an end-to-end server test. Microsoft device-code sign-in/entitlement is optional and remains incomplete.
- SAF folder selection is not implemented; current game data and saves remain in app-private storage.
- Never create an APK unless the maintainer explicitly requests one.

## Remaining integration sequence

1. Run `.github/workflows/android-jdk25.yml` and confirm that the Android ARM64
   JDK release assets are published; then test Java/Javac execution on Android.
2. Complete the Android shell CI and resolve any Kotlin, Gradle, Rust or native
   packaging errors.
3. Test Mojang downloads and full resource preparation on the phone, including
   process permissions, cancellation, resume/retry and the final `--check`.
4. On a real Android ARM64 device, confirm NativeActivity, Winit, WGPU surface
   creation, the title frame and the `game_client_ready` handoff.
5. Connect the existing touch-control module to Winit touch events, HUD drawing
   and gameplay input; test multitouch, movement, camera, attack and inventory.
6. Test creating, saving, reopening and entering a single-player world, then
   validate lifecycle, Android Back, focus changes, memory use and performance.
7. Wire Android audio output and test sound. Add SAF storage only if external
   world-folder import/export is required.
8. Test Ely.by end-to-end against a compatible server. Add Microsoft device-code
   login and entitlement validation only for Microsoft online-mode servers.
