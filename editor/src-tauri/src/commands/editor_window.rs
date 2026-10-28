//! Editor window / webview plumbing.
//!
//! The editor runs inside WebView2, whose browser accelerator keys (F3
//! find-next, Ctrl+F, Ctrl+R, F5, F12, Ctrl+P, …) are handled by the
//! browser before the page can act on them — so editor shortcuts bound to
//! those keys never fired. [`disable_browser_accelerator_keys`] turns that
//! handling off; the key events still reach the page, so the frontend
//! re-implements the ones worth keeping (reload, devtools) itself and
//! surfaces them in the Debug menu.

/// Open (or close) the webview devtools for the calling window.
///
/// Replaces the F12 / Ctrl+Shift+I browser accelerators that
/// [`disable_browser_accelerator_keys`] switches off. Requires the `devtools`
/// Cargo feature, which this crate enables unconditionally.
#[tauri::command]
pub fn toggle_devtools(window: tauri::WebviewWindow) {
    if window.is_devtools_open() {
        window.close_devtools();
    } else {
        window.open_devtools();
    }
}

/// Turn off WebView2's browser accelerator keys for `window`.
///
/// Mirrors what wry's `with_browser_accelerator_keys(false)` does at build
/// time — Tauri does not expose that builder option, so we reach the
/// `ICoreWebView2Settings3` interface through `with_webview` instead.
/// Editing keys (Ctrl+C/V/X/A/Z) are a separate WebView2 category and stay
/// enabled. Best-effort: a failure is logged and leaves the defaults in
/// place rather than blocking startup.
#[cfg(windows)]
pub fn disable_browser_accelerator_keys(window: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows::core::Interface;

    let result = window.with_webview(|webview| {
        let apply = || -> Result<(), windows::core::Error> {
            let settings = unsafe { webview.controller().CoreWebView2()?.Settings()? };
            let settings3: ICoreWebView2Settings3 = settings.cast()?;
            unsafe { settings3.SetAreBrowserAcceleratorKeysEnabled(false) }
        };
        match apply() {
            Ok(()) => log::info!("webview: browser accelerator keys disabled"),
            Err(e) => log::warn!("webview: could not disable browser accelerator keys: {e}"),
        }
    });
    if let Err(e) = result {
        log::warn!("webview: with_webview failed: {e}");
    }
}

/// No-op on non-Windows hosts — the accelerator-key clash is WebView2-specific.
#[cfg(not(windows))]
pub fn disable_browser_accelerator_keys(_window: &tauri::WebviewWindow) {}

/// The WebKit feature that holds a page's `requestAnimationFrame` near 60 a
/// second on any display - Safari's Develop > Feature Flags "Prefer Page
/// Rendering Updates near 60fps". On by default.
#[cfg(target_os = "macos")]
const NEAR_60_FPS_FEATURE: &str = "PreferPageRenderingUpdatesNear60FPSEnabled";

/// Let the editor's page draw at the display's own rate on macOS.
///
/// WKWebView holds `requestAnimationFrame` near 60 a second even on a 120 Hz
/// ProMotion panel, so the viewport - and a game in Play - ran at 60 on a Mac
/// where native runs at 120. The switch is a WebKit feature flag reached
/// through `WKPreferences`' feature list, which is private API: fine for a
/// development tool, and every call is guarded, so a WebKit without it simply
/// keeps 60. The webview's configuration is read live (`preferences` of its
/// `configuration`), and the page is drawing already, which the flag takes
/// effect on.
#[cfg(target_os = "macos")]
pub fn allow_full_frame_rate(window: &tauri::WebviewWindow) {
    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{msg_send, sel};

    fn responds(target: &AnyObject, selector: Sel) -> bool {
        let yes: Bool = unsafe { msg_send![target, respondsToSelector: selector] };
        yes.as_bool()
    }
    fn nsstring_eq(s: *mut AnyObject, want: &str) -> bool {
        if s.is_null() {
            return false;
        }
        let utf8: *const std::ffi::c_char = unsafe { msg_send![&*s, UTF8String] };
        !utf8.is_null() && unsafe { std::ffi::CStr::from_ptr(utf8) }.to_bytes() == want.as_bytes()
    }

    let result = window.with_webview(|webview| unsafe {
        let wk = webview.inner() as *mut AnyObject;
        let Some(class) = AnyClass::get(c"WKPreferences") else { return };
        let class: &AnyObject = &*(class as *const AnyClass as *const AnyObject);
        // `_features` on current WebKit; `_internalDebugFeatures` on older.
        let list = [sel!(_features), sel!(_internalDebugFeatures)]
            .into_iter()
            .find(|s| responds(class, *s));
        let Some(list) = list else {
            log::info!("webview: WebKit has no feature list; frame rate stays near 60");
            return;
        };
        let features: *mut AnyObject = if list == sel!(_features) {
            msg_send![class, _features]
        } else {
            msg_send![class, _internalDebugFeatures]
        };
        let configuration: *mut AnyObject = msg_send![&*wk, configuration];
        let preferences: *mut AnyObject = msg_send![&*configuration, preferences];
        if features.is_null() || preferences.is_null() {
            return;
        }
        let setter = [sel!(_setEnabled:forFeature:), sel!(_setEnabled:forInternalDebugFeature:)]
            .into_iter()
            .find(|s| responds(&*preferences, *s));
        let Some(setter) = setter else { return };
        let count: usize = msg_send![&*features, count];
        for i in 0..count {
            let feature: *mut AnyObject = msg_send![&*features, objectAtIndex: i];
            let key: *mut AnyObject = msg_send![&*feature, key];
            if nsstring_eq(key, NEAR_60_FPS_FEATURE) {
                if setter == sel!(_setEnabled:forFeature:) {
                    let _: () = msg_send![&*preferences, _setEnabled: Bool::NO, forFeature: &*feature];
                } else {
                    let _: () = msg_send![&*preferences, _setEnabled: Bool::NO, forInternalDebugFeature: &*feature];
                }
                log::info!("webview: {NEAR_60_FPS_FEATURE} switched off - the page draws at the display's rate");
                return;
            }
        }
        log::info!("webview: no {NEAR_60_FPS_FEATURE} feature; frame rate stays near 60");
    });
    if let Err(e) = result {
        log::warn!("webview: with_webview failed: {e}");
    }
}

/// No-op off macOS - the 60 fps hold is WebKit's.
#[cfg(not(target_os = "macos"))]
pub fn allow_full_frame_rate(_window: &tauri::WebviewWindow) {}
