# Horizon Rust Mobile — Android port plan

This document tracks the Android port of the existing Horizon engine. The goal is to run the same Rust game on Android, not to replace it with a Java Edition launcher or a separate game implementation.

## Non-negotiable principles

- Keep the upstream Rust game and its vanilla-parity behavior as the source of truth.
- Keep Android-specific code behind a narrow platform boundary; do not fork gameplay rules or duplicate the renderer.
- Do not bundle Mojang game jars, assets, or other copyrighted game data. The player must provide data from their own legally obtained Minecraft: Java Edition copy, following Horizon's existing preparation flow.
- Do not claim Android support until a real device build has launched into a world and passed the relevant checks.
- Do not produce or publish an APK until the maintainer explicitly requests it.

## Target and compatibility

- First target: Android 10+ on arm64-v8a.
- First validation device: Samsung Galaxy A12 (SM-A125M), Android 12, ARM64, PowerVR GE8320.
- Detect Vulkan and device limits at runtime. Do not assume that every Android GPU/driver exposes the required Vulkan features.
- Record unsupported GPU/driver combinations clearly; never silently fall back to a renderer that Horizon does not actually support.
- Treat RAM, thermal limits, backgrounding, and Android process lifecycle as first-class constraints.

## Porting phases

### 0. Baseline and architecture audit

- Record the exact upstream commit and the fork's delta.
- Map the game entry point, window/surface creation, wgpu instance/device creation, input loop, audio, filesystem paths, and launcher/game boundary.
- Identify every desktop-only dependency and every platform conditional that blocks Android.
- Inspect existing Android-related upstream work before adding parallel implementations.
- Establish baseline cargo check, formatting, lint, unit-test, and parity results on supported desktop CI.

**Exit criteria:** a reviewed dependency map and a list of minimal Android-specific seams, with no speculative source changes.

### 1. Native Android runtime

- Add an Android build target for aarch64-linux-android and a reproducible NDK toolchain in CI.
- Build the engine as an Android-loadable native library or executable using the smallest bridge compatible with the existing game entry point.
- Implement Android lifecycle handling: create/resume/pause/stop/destroy, surface loss/recreation, and safe shutdown.
- Route logs and crash diagnostics to app-accessible storage.
- Verify that no desktop-only filesystem path or process assumption leaks into Android.

**Exit criteria:** native library loads on a real arm64 Android device, reports device/GPU information, survives pause/resume, and exits cleanly. This is not yet a playable build.

### 2. Rendering and input

- Connect the Android window surface to wgpu's supported Android Vulkan path.
- Recreate the swapchain safely after rotation, pause/resume, and surface replacement.
- Add touch input as a platform adapter while preserving keyboard/controller input.
- Add an on-screen keyboard and configurable touch controls.
- Validate frame pacing, GPU limits, texture formats, depth formats, and device-loss handling on real hardware.

**Exit criteria:** a stable rendered test scene on a real Android device with touch input and lifecycle tests passing.

### 3. Mobile game experience

- Add a portrait-capable mobile shell and a landscape-first in-game layout.
- Use animated bottom navigation tabs for Home, Versions, Mods, and Settings.
- Keep the in-game HUD and touch controls separate from launcher navigation.
- Add safe-area handling, scalable typography, and responsive layouts.
- Provide accessible controls and configurable button placement/size.

**Exit criteria:** navigation and gameplay controls work at common phone resolutions and orientations without covering essential game UI.

### 4. Files, accounts, and game data

- Use Android's Storage Access Framework for user-selected files and folders; request only the access actually needed.
- Keep app-private data as the default location, with a user-selectable game directory where Android permissions allow it.
- Reuse Horizon's existing authentication and preparation logic where platform-compatible.
- Never ship Minecraft assets or bypass ownership checks. Let the player select/import their own game data or use an officially supported account flow.
- Make downloads resumable, integrity-checked, cancellable, and safe across process death.

**Exit criteria:** the app can prepare a valid user-owned game-data directory and verify every required file before launch.

### 5. Performance and reliability

- Profile CPU, GPU, memory, startup time, frame pacing, and thermal behavior on low-end and mid-range Android devices.
- Use measured quality presets and conservative defaults; do not promise FPS gains without device benchmarks.
- Bound worker-thread counts to Android's available cores and memory budget.
- Handle low-memory callbacks, backgrounding, interrupted downloads, storage loss, and GPU/device loss.
- Preserve diagnostic logs and offer a share/copy crash report action.

**Exit criteria:** repeatable device benchmarks, no known lifecycle crash, and documented minimum requirements.

## Validation checklist

- [ ] Android Gradle shell build on latest commit (CI running; earlier shell build passed)
- [ ] cargo fmt --check
- [ ] cargo check for the Android target (ARM64 cross-check workflow added; result pending)
- [ ] Android native library loads on arm64 hardware
- [ ] Vulkan capability and extension checks are explicit
- [ ] Surface recreation works after pause/resume and rotation
- [ ] Touch, keyboard, and controller input paths are tested
- [ ] Storage permissions and SAF flows are tested on Android 12+
- [ ] Game-data integrity checks reject missing or modified files
- [ ] No Mojang assets or prepared game data are committed
- [ ] Memory, thermal, and frame-time benchmarks are recorded
- [ ] Existing desktop builds and vanilla-parity tests remain green

## Current status

Android shell and landscape orientation are implemented. A new `horizon-android` `cdylib` entry point uses Android `NativeActivity`, passes `AndroidApp` into winit's Android event-loop builder, and routes both the normal game loop and the missing-resources screen through the Android-aware event-loop factory. The existing Horizon `winit` window is then used by its normal `wgpu` renderer to acquire the Android surface; no separate fake/placeholder renderer was added. The Android CI workflow now cross-builds the ARM64 native library and stages it into Gradle's `jniLibs` before assembling the shell. This build and packaging path is still pending CI verification and real-device surface testing. The app now resolves the pinned Minecraft 26.3 Mojang metadata and downloads the version JSON, client/server jars, Maven artifacts, and asset objects with SHA-1/size verification, resumable partial files, progress, and cancellation. This downloads game inputs only: Microsoft ownership validation, Android-compatible Java 25 resource preparation, the ARM64 Rust runtime/bridge, touch/render integration, and real-device gameplay are still incomplete. No playable Android port or APK is claimed. Each phase must be implemented and tested before it is marked complete.