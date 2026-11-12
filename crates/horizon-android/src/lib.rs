//! Android NativeActivity entry point. Winit owns the Android surface and event lifecycle;
//! Horizon's normal game renderer then creates its wgpu surface from the winit Window.

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[global_allocator]
static ALLOCATOR: horizon_threads::GameAllocator = horizon_threads::GAME_ALLOCATOR;

/// Entry point loaded by Android NativeActivity (see mobile/app/src/main/AndroidManifest.xml).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    // Keep all mutable game state and prepared resources inside this app's private storage.
    let data_dir = app.internal_data_path().unwrap_or_else(|| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&data_dir);
    let _ = std::env::set_current_dir(&data_dir);

    let home = data_dir.join("horizon-home");
    let player_dir = data_dir.join("horizon-player");
    let resources = data_dir.join(".local").join("horizon-client").join("26.3");
    let mut args = vec![
        "--home".to_owned(), home.to_string_lossy().into_owned(),
        "--player-dir".to_owned(), player_dir.to_string_lossy().into_owned(),
        "--resources".to_owned(), resources.to_string_lossy().into_owned(),
    ];
    // The Android launcher stores only the public offline player name. With no
    // session file, Horizon uses its normal offline UUID and never claims online auth.
    let offline_name = data_dir.join("horizon-player-name.txt");
    if let Ok(name) = std::fs::read_to_string(&offline_name) {
        let name = name.trim();
        if !name.is_empty()
            && name.len() <= 16
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            args.push("--name".to_owned());
            args.push(name.to_owned());
        }
    }
    // A fresh one-use Ely.by session is created by MainActivity immediately
    // before opening NativeActivity. AuthSession consumes and deletes it.
    let session_file = data_dir.join("horizon-session.json");
    if session_file.is_file() {
        args.push("--session-file".to_owned());
        args.push(session_file.to_string_lossy().into_owned());
    }

    horizon_client::set_android_app(app);
    horizon_client::entry::main_with_args(Default::default(), args);
}
