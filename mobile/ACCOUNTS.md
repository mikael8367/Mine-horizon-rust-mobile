# Android account providers

The Android launcher supports two selectable player modes:

- **Offline**: stores only the player name in app-private storage and starts Horizon with `--name`. The game uses its normal offline UUID. This is for single-player and servers configured to accept offline-mode players; it does not authenticate an online-mode server.
- **Ely.by**: authenticates against Ely.by's Yggdrasil endpoint over HTTPS. The password is not persisted. The access token and profile are encrypted at rest with an Android Keystore AES-GCM key. Just before launching NativeActivity, the launcher writes a short-lived, owner-only session file; the Rust client consumes and deletes it. The game sends the Yggdrasil `/session/join` request to Ely.by when joining an online-mode server.

## Ely.by compatibility notes

- An online-mode server must be configured with Ely.by-compatible authentication (for example, the server-side Authlib integration). A server that only trusts Mojang/Microsoft sessions will reject Ely.by accounts.
- Ely.by chat profile-key signing is not implemented in this Android integration. Servers enforcing secure chat may limit chat for Ely.by sessions.
- If the Ely.by token expires or is revoked, sign in again in the launcher. Automatic Yggdrasil token refresh and two-factor-code UI are not included yet.
- Microsoft login is intentionally not required. It remains a separate optional provider and is not enabled by this Android account screen.

The account session file is temporary and is not intended to be copied, backed up, or shared. Never put account passwords or access tokens in issue reports or logs.
