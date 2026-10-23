//! The Help menu's side: what the About box says about this build, and
//! opening the documentation, the issue tracker and the log folder.

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// What About jEngine shows.
#[derive(Debug, Clone, Serialize)]
pub struct AboutInfo {
    /// The workspace version (`[workspace.package] version`), not the
    /// frontend's `package.json`.
    pub version: String,
    /// Short commit hash, or `unknown` when the build had no git.
    pub commit: String,
    /// `YYYY-MM-DD`, UTC.
    pub build_date: String,
    /// `debug` or `release`.
    pub profile: String,
    pub os: String,
    pub arch: String,
    pub tauri: String,
}

#[tauri::command]
pub fn about_info() -> AboutInfo {
    AboutInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        commit: env!("JENGINE_GIT_COMMIT").to_string(),
        build_date: env!("JENGINE_BUILD_DATE").to_string(),
        profile: if cfg!(debug_assertions) { "debug" } else { "release" }.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        tauri: tauri::VERSION.to_string(),
    }
}

/// Only web pages: this is handed to the shell, so a `file:` or custom
/// scheme from anywhere would be a way to start a program.
fn is_web_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c == '"' || c.is_control())
}

/// Open a web page in the system browser.
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    if !is_web_url(&url) {
        return Err(format!("'{url}' is not a web address"));
    }
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // The URL protocol handler, not `cmd /C start`: cmd would read an
        // `&` in a query string as the end of the command.
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", &url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| format!("Could not open a browser: {e}"))
}

/// Where `tauri_plugin_log` writes the editor's log (its `LogDir` target,
/// on by default): `<app log dir>/<product name>.log`.
#[tauri::command]
pub fn logs_folder(app: AppHandle) -> Result<String, String> {
    app.path().app_log_dir().map(|p| p.display().to_string()).map_err(|e| e.to_string())
}

/// Open the editor's log folder in the system file manager.
#[tauri::command]
pub fn open_logs_folder(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    // A fresh install has logged nothing yet on some platforms.
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open {}: {e}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_pages_are_opened() {
        assert!(is_web_url("https://jengine.hirakame.com/api-reference/scripting-api/"));
        assert!(is_web_url("https://github.com/jkilpala/jengine/issues/new/choose"));
        assert!(!is_web_url("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_web_url("C:\\Windows\\notepad.exe"));
        assert!(!is_web_url("https://example.com/a b"));
        assert!(!is_web_url("https://example.com/\" & calc"));
    }

    #[test]
    fn the_about_box_knows_its_build() {
        let about = about_info();
        assert_eq!(about.version, env!("CARGO_PKG_VERSION"));
        assert!(!about.commit.is_empty());
        let date = about.build_date.as_bytes();
        assert_eq!(date.len(), 10, "{}", about.build_date);
        assert_eq!((date[4], date[7]), (b'-', b'-'));
    }
}
