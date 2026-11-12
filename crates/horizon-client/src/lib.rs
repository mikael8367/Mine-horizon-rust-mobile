//! Gameplay and startup, above the reusable client library.
// No `#![recursion_limit]` or other inner attribute horizon-mixins doesn't know here: the
// launcher's horizon-mixins then rejects every native mod targeting this crate (#8169).
#![forbid(unsafe_code)]
extern crate self as horizon_client;
pub use horizon_client_lib::*;
pub mod entry;
pub mod game;
mod horizon_look;
pub mod mod_api;

/// Supplies the Android application handle to winit before the native game loop starts.
#[cfg(target_os = "android")]
thread_local! {
    static ANDROID_APP: std::cell::RefCell<Option<winit::platform::android::activity::AndroidApp>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "android")]
pub fn set_android_app(app: winit::platform::android::activity::AndroidApp) {
    ANDROID_APP.with(|slot| *slot.borrow_mut() = Some(app));
}

#[cfg(target_os = "android")]
pub(crate) fn android_app() -> Option<winit::platform::android::activity::AndroidApp> {
    ANDROID_APP.with(|slot| slot.borrow().clone())
}

/// Builds the event loop with the platform handle required by winit on Android.
#[cfg(target_os = "android")]
pub fn create_event_loop() -> std::result::Result<winit::event_loop::EventLoop<()>, winit::error::EventLoopError> {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    let app = android_app().expect("Android entry point must provide AndroidApp before creating a window");
    let mut builder = winit::event_loop::EventLoop::builder();
    builder.with_android_app(app);
    builder.build()
}

/// Desktop event-loop construction remains unchanged.
#[cfg(not(target_os = "android"))]
pub fn create_event_loop() -> std::result::Result<winit::event_loop::EventLoop<()>, winit::error::EventLoopError> {
    winit::event_loop::EventLoop::new()
}

#[cfg(test)]
mod brewing_speed_load_tests;
#[cfg(test)]
mod living_death_tests;
