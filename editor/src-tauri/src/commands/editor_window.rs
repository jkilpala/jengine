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
