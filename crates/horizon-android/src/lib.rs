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
    // Keep Horizon's default game directory inside this app's private storage.
    if let Some(data_dir) = app.internal_data_path() {
        let _ = std::fs::create_dir_all(&data_dir);
        let _ = std::env::set_current_dir(data_dir);
    }
    horizon_client::set_android_app(app);
    horizon_client::entry::main();
}
