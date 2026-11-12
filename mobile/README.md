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

This is a verified file-download stage, not yet a prepared or playable game.
Microsoft sign-in and Java Edition entitlement validation must be integrated
before preparation/Play can be enabled. The app currently has no Play action.

## Runtime and preparation

Horizon gameplay is Rust-native; it does not need a desktop Java VM to execute
the game. Android still needs a compatible ARM64 native engine/host bridge,
its native dependencies, and a verified runtime package. No such Android runtime
artifact is published yet, so the runtime section intentionally reports pending
instead of downloading an incompatible desktop binary.

The existing `horizon-prepare` pipeline also invokes Java 25 for vanilla resource
exporters. That is a separate preparation requirement, not the runtime used to
execute Rust gameplay. The desktop Mojang Linux JRE is not an Android runtime.
An Android-compatible preparation strategy and runtime must be implemented and
tested before full game-data preparation can be claimed.

## Current implementation status

- Android app shell is landscape-first and uses immersive mode.
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
  prepared and does not enable Play.
- ARM64 Android target cross-check workflow added; app-loadable native engine packaging, Microsoft device-code sign-in/entitlement,
  Java-25-compatible resource preparation, SAF folder selection, and gameplay
  integration remain incomplete.
- Never create an APK unless the maintainer explicitly requests one.

## Remaining integration sequence

1. Add Microsoft device-code sign-in and Minecraft: Java Edition entitlement validation.
2. Build the ARM64 Android engine and native bridge in CI; publish a signed/checksummed
   runtime manifest and compatible native package.
3. Implement an Android-compatible Java 25 preparation strategy for Horizon's
   vanilla exporters, or replace those exporter calls with a verified Android-native path.
4. Validate all prepared resources using Horizon's resource checker.
5. Enable Play only after account entitlement, game data, prepared resources and
   native runtime all pass validation.
6. Add Android Storage Access Framework support and real-device lifecycle/performance tests.
