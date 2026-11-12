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
    let startup_status = data_dir.join("horizon-startup-status.txt");
    let write_status = |stage: &str, detail: &str| {
        let body = format!("{stage}\n{detail}\n");
        let temp = data_dir.join("horizon-startup-status.txt.part");
        if std::fs::write(&temp, body).is_ok() {
            let _ = std::fs::rename(&temp, &startup_status);
        }
    };
    write_status("native_entry_started", "NativeActivity chamou android_main.");

    let home = data_dir.join("horizon-home");
    let game_dir = data_dir.join("horizon-game");
    let player_dir = data_dir.join("horizon-player");
    let resources = data_dir.join(".local").join("horizon-client").join("26.3");
    for directory in [&home, &game_dir, &player_dir, &resources] {
        if let Err(error) = std::fs::create_dir_all(directory) {
            write_status("storage_setup_failed", &format!(
                "Não foi possível preparar {}: {error}", directory.display()
            ));
            return;
        }
    }
    let resource_manifest = resources.join("horizon-resources.json");
    let prepare_manifest = resources.join("horizon-prepare.json");
    let resources_ready = [&resource_manifest, &prepare_manifest]
        .into_iter()
        .all(|path| {
            std::fs::metadata(path)
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 2)
        });
    if !resources_ready {
        write_status(
            "preflight_failed",
            "Manifestos de recursos ausentes ou vazios. O Horizon continuará para exibir seu diagnóstico de recursos.",
        );
        // Let the engine's own missing/damaged-resource screen explain the problem.
    } else {
        write_status("native_preflight_ok", "Os dois manifestos de recursos foram encontrados.");
    }

    let mut args = vec![
        "--home".to_owned(), home.to_string_lossy().into_owned(),
        "--game-dir".to_owned(), game_dir.to_string_lossy().into_owned(),
        "--player-dir".to_owned(), player_dir.to_string_lossy().into_owned(),
        "--resources".to_owned(), resources.to_string_lossy().into_owned(),
    ];
    // The Android launcher stores only the public offline player name. With no
    // session file, Horizon uses its normal offline UUID and never claims online auth.
    let offline_name = data_dir.join("horizon-player-name.txt");
    if let Ok(name) = std::fs::read_to_string(&offline_name) {
        let name = name.trim();
        if !name.is_empty()
            && (3..=16).contains(&name.len())
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

    write_status("engine_starting", "Entrando no loop principal do Horizon.");
    horizon_client::set_android_app(app);
    horizon_client::entry::main_with_args(Default::default(), args);
}
