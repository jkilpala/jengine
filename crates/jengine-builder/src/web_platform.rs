//! Web portals: the SDK and the adapter a web build ships for the portal it
//! is made for (`docs/web-platforms.md`).
//!
//! A portal - Poki, CrazyGames, YouTube Playables, jCabinet - wants to hear
//! the same few things from every game (loading done, play started and
//! stopped, an ad please) through its own SDK, and some can pause or mute the
//! game. The engine says them once, through `window.__jengine_platform`
//! (`Platform.*` in scripts, `jengine-ready` from the runtime, `paused` and
//! `muted` read back every frame); the adapter in `assets/platform/` turns
//! that into one portal's calls. The portal is chosen per build - the same
//! game goes to several - from `--platform`, else the project's
//! `settings.web_platform`.

use std::path::Path;

/// The portals a web build can be made for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WebPlatform {
    /// No portal: no SDK, no adapter; `Platform.*` answer as natively.
    #[default]
    None,
    /// jCabinet (jWorld, FingerCorner): the in-house portal.
    JCabinet,
    Poki,
    CrazyGames,
    /// YouTube Playables.
    YouTube,
}

impl WebPlatform {
    pub const ALL: [WebPlatform; 5] = [
        WebPlatform::None,
        WebPlatform::JCabinet,
        WebPlatform::Poki,
        WebPlatform::CrazyGames,
        WebPlatform::YouTube,
    ];

    /// The id settings, the CLI and `Platform.name()` use.
    pub fn id(self) -> &'static str {
        match self {
            WebPlatform::None => "none",
            WebPlatform::JCabinet => "jcabinet",
            WebPlatform::Poki => "poki",
            WebPlatform::CrazyGames => "crazygames",
            WebPlatform::YouTube => "youtube",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|p| p.id() == text)
    }

    /// The portal's SDK, loaded before anything else on the page.
    pub fn sdk_url(self) -> Option<&'static str> {
        match self {
            WebPlatform::None => None,
            // Served by the portal itself, so it works on every jCabinet site.
            WebPlatform::JCabinet => Some("/api/v1/sdk/jcabinet.js"),
            WebPlatform::Poki => Some("https://game-cdn.poki.com/scripts/v2/poki-sdk.js"),
            WebPlatform::CrazyGames => Some("https://sdk.crazygames.com/crazygames-sdk-v3.js"),
            WebPlatform::YouTube => Some("https://www.youtube.com/game_api/v1"),
        }
    }

    fn adapter_source(self) -> Option<&'static str> {
        match self {
            WebPlatform::None => None,
            WebPlatform::JCabinet => Some(include_str!("../assets/platform/jcabinet.js")),
            WebPlatform::Poki => Some(include_str!("../assets/platform/poki.js")),
            WebPlatform::CrazyGames => Some(include_str!("../assets/platform/crazygames.js")),
            WebPlatform::YouTube => Some(include_str!("../assets/platform/youtube.js")),
        }
    }
}

/// What everything every adapter shares does (`assets/platform/core.js`).
const CORE_JS: &str = include_str!("../assets/platform/core.js");

/// The file the page loads after the SDK.
pub const ADAPTER_FILE: &str = "jengine-platform.js";

/// Share of a portal's loading bar that is the wasm download; the files the
/// engine fetches before its first frame fill the rest.
const WASM_SHARE: f64 = 0.7;

/// When the portal hears the game is ready to play (YouTube's `gameReady`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GameReady {
    /// On the first frame - right for a game that opens on its menu.
    #[default]
    FirstFrame,
    /// When a script calls `Platform.game_ready()` - for a game with a splash
    /// or an intro first, which YouTube must not be told is playable.
    Script,
}

impl GameReady {
    pub fn id(self) -> &'static str {
        match self {
            GameReady::FirstFrame => "first_frame",
            GameReady::Script => "script",
        }
    }
}

/// The project's portal settings: `settings.web_platform` and
/// `settings.web_game_ready` in `project.jengine`. An unknown portal is a
/// warning and no portal, not a failed build.
pub fn read_settings(project_path: &Path) -> (WebPlatform, GameReady) {
    let doc = std::fs::read_to_string(project_path.join("project.jengine"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
    let settings = doc.as_ref().and_then(|d| d.get("settings"));
    let platform = match settings.and_then(|s| s.get("web_platform")).and_then(|v| v.as_str()) {
        None | Some("") => WebPlatform::None,
        Some(id) => WebPlatform::parse(id).unwrap_or_else(|| {
            log::warn!("settings.web_platform \"{id}\" is not a known portal ({}); building for none", known_ids());
            WebPlatform::None
        }),
    };
    let game_ready = match settings.and_then(|s| s.get("web_game_ready")).and_then(|v| v.as_str()) {
        Some("script") => GameReady::Script,
        _ => GameReady::FirstFrame,
    };
    (platform, game_ready)
}

/// `none, jcabinet, poki, ...` - for messages.
pub fn known_ids() -> String {
    WebPlatform::ALL.iter().map(|p| p.id()).collect::<Vec<_>>().join(", ")
}

/// The `<script>` tags for the page's `<head>`: the SDK, then the adapter.
/// Classic scripts, so both have run before the module script starts the
/// wasm. Empty for no portal.
pub fn head_tags(platform: WebPlatform) -> String {
    let Some(sdk) = platform.sdk_url() else {
        return String::new();
    };
    format!(
        "  <!-- Web portal: {id} (docs/web-platforms.md). Its SDK, then the adapter the engine talks to. -->\n  <script src=\"{sdk}\"></script>\n  <script src=\"./{ADAPTER_FILE}\"></script>\n",
        id = platform.id()
    )
}

/// `jengine-platform.js` for a portal: its adapter and the shared core, with
/// the build's facts in `CONFIG`. `None` for no portal.
pub fn adapter_js(platform: WebPlatform, game_ready: GameReady, wasm_file: &str, wasm_bytes: u64) -> Option<String> {
    let portal = platform.adapter_source()?;
    let config = serde_json::json!({
        "platform": platform.id(),
        "gameReady": game_ready.id(),
        "wasm": wasm_file,
        "wasmBytes": wasm_bytes.max(1),
        "wasmShare": WASM_SHARE,
    });
    Some(format!(
        "// jengine-platform.js - written by `jengine web-build` for {id}; see docs/web-platforms.md.\n(function () {{\n  'use strict';\n  var CONFIG = {config};\n\n{portal}\n{CORE_JS}\n}})();\n",
        id = platform.id(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for p in WebPlatform::ALL {
            assert_eq!(WebPlatform::parse(p.id()), Some(p));
        }
        assert_eq!(WebPlatform::parse(" Poki "), Some(WebPlatform::Poki));
        assert_eq!(WebPlatform::parse("itch"), None);
    }

    #[test]
    fn no_portal_adds_nothing() {
        assert!(head_tags(WebPlatform::None).is_empty());
        assert!(adapter_js(WebPlatform::None, GameReady::FirstFrame, "x_bg.wasm", 1).is_none());
    }

    #[test]
    fn every_portal_has_an_sdk_and_an_adapter() {
        for p in WebPlatform::ALL.into_iter().filter(|p| *p != WebPlatform::None) {
            let tags = head_tags(p);
            assert!(tags.contains(p.sdk_url().unwrap()), "{p:?}: {tags}");
            assert!(tags.contains(ADAPTER_FILE));
            let js = adapter_js(p, GameReady::Script, "jengine_web_bg.wasm", 1234).unwrap();
            assert!(js.contains(&format!("name: '{}'", p.id())), "{p:?} adapter names itself");
            assert!(js.contains("\"gameReady\":\"script\""));
            assert!(js.contains("\"wasmBytes\":1234"));
            assert!(js.contains("window.__jengine_platform = api"));
        }
    }

    #[test]
    fn settings_are_read_from_the_project() {
        let dir = std::env::temp_dir().join(format!("jengine-web-platform-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("project.jengine"),
            r#"{ "settings": { "web_platform": "crazygames", "web_game_ready": "script" } }"#,
        )
        .unwrap();
        assert_eq!(read_settings(&dir), (WebPlatform::CrazyGames, GameReady::Script));
        std::fs::write(dir.join("project.jengine"), r#"{ "settings": { "web_platform": "itch" } }"#).unwrap();
        assert_eq!(read_settings(&dir), (WebPlatform::None, GameReady::FirstFrame));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
