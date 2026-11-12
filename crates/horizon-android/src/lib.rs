//! Android NativeActivity entry point. Winit owns the Android surface and event lifecycle;
//! Horizon's normal game renderer then creates its wgpu surface from the winit Window.

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

/// Entry point loaded by Android NativeActivity (see mobile/app/src/main/AndroidManifest.xml).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    horizon_client::set_android_app(app);
    horizon_client::entry::main();
}
