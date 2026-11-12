# Horizon Rust Mobile — Android module

This is the Android host module for the existing Horizon Rust game. It is not a
standalone replacement game or a Java Edition launcher.

## Download requirements

### Minecraft game data

The mobile installer must resolve the pinned Minecraft version through Mojang's
official version manifest and download only from Mojang's distribution hosts.
Every artifact must be checked against the SHA-1 and size in the official version
metadata. Asset-index objects must be checked individually. Downloads must be
resumable, cancellable, and staged as `.part` files; an incomplete or invalid
file must never be exposed as installed game data.

The player must sign in with a Microsoft account that owns Minecraft: Java
Edition before the official client data is prepared/launched. Do not bundle
Minecraft jars/assets in this repository, APK, or CI artifacts, and do not use
third-party mirrors.

### Horizon native runtime

Horizon gameplay is Rust-native; it does not need a desktop Java VM to execute
the game. Android needs an ARM64 native engine package (the Android-compatible
Rust shared library/host bridge, native dependencies, and verified metadata).
Install it into app-private storage, verify its release checksum/signature, and
activate it atomically only after all required files pass verification.

The existing `horizon-prepare` pipeline also invokes Java for vanilla resource
exporters. That is a separate preparation requirement, not the runtime used to
execute the Rust game. An Android-compatible preparation strategy/runtime must
be implemented and tested before claiming full game-data preparation on Android.

## Current implementation status

- `VerifiedDownloader.kt` provides HTTPS allow-listed, SHA-1/SHA-256 verified
  downloads, redirects restricted to trusted distribution hosts, resume support,
  progress callbacks, temporary files, and atomic finalization.
- This helper is infrastructure only. It is not yet wired to a Minecraft
  version picker, Mojang account authentication, asset-index traversal, the
  Android native runtime release feed, or the play button.
- No Android native Horizon runtime artifact is published by this module yet.
- Do not show a download as complete until the metadata, all required files,
  hashes, and final install validation pass.
- Never create an APK unless the maintainer explicitly requests one.

## Required integration sequence

1. Add the Microsoft device-code authentication and entitlement check.
2. Fetch and pin the official Mojang version manifest for the Horizon-required version.
3. Download the version JSON, client/server inputs, libraries, and asset objects with
   official hashes; record total progress and allow cancellation.
4. Build the ARM64 Android engine and bridge in CI, publish a checksum/signature
   manifest, and download that exact compatible build.
5. Run Android-compatible resource preparation; validate the output with the
   engine's resource checker.
6. Enable Play only after native runtime, owned game data, and prepared resources
   all pass validation.
