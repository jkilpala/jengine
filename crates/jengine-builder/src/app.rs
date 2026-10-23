//! What a game is called on a phone: its id, name, version, icon and
//! orientation.
//!
//! `project.jengine` carries these under `settings.app`, shared by every
//! platform that installs an app (Android now, iOS next), with
//! Android-only numbers under `settings.android`:
//!
//! ```jsonc
//! "settings": {
//!   "app": {
//!     "id": "com.example.arcade",      // Android package name / iOS bundle id
//!     "name": "Example Arcade",        // under the icon; default: settings.title
//!     "version": "1.2.0",              // what a player sees
//!     "build": 7,                      // must go up with every store upload
//!     "icon": "icon.png",              // square PNG, 512 px or more, project-relative
//!     "icon_background": "#10141c",    // behind the icon where the launcher masks it
//!     "orientation": "landscape"       // landscape | portrait | any
//!   },
//!   "android": { "min_sdk": 26, "target_sdk": 36, "abis": ["arm64-v8a"],
//!                "immersive": true,              // hide the system bars
//!                "keystore": "release.jks",      // the release key, for a store
//!                "key_alias": "upload" }         // (passwords never live here)
//! }
//! ```
//!
//! Every field may be left out. [`AppSettings::resolve`] fills the gaps and
//! is the only place defaults live, so the editor can show what a build will
//! actually use.

use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// `settings.app` as authored. Empty strings and zero mean "not set".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Reverse-domain id: the Android package name and the iOS bundle id.
    /// It is the app's identity in a store and cannot change after release.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The name under the icon. Defaults to `settings.title`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The version players see (`versionName`, `CFBundleShortVersionString`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// The number stores compare (`versionCode`, `CFBundleVersion`): every
    /// upload needs a higher one than the last.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub build: u32,
    /// Square PNG, relative to the project root. Kept out of `assets/` by
    /// preference, since nothing in the game draws it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// `#rrggbb` shown around the icon where a launcher crops it to a circle
    /// or squircle.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon_background: String,
    /// `landscape`, `portrait` or `any`. Defaults to the shape of the
    /// project's `width` x `height`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub orientation: String,
}

/// `settings.android` as authored. Zero and empty mean "not set".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AndroidSettings {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min_sdk: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub target_sdk: u32,
    /// `arm64-v8a` (every phone since ~2017) and / or `x86_64` (the
    /// emulator on a PC). More ABIs mean a longer build and a bigger APK.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abis: Vec<String>,
    /// Hide the status and navigation bars while the game has focus - what a
    /// game wants, so the default. Needs a few lines of Java in the APK
    /// (`assets/android/JEngineActivity.java`); `false` ships NativeActivity
    /// itself and no code at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub immersive: Option<bool>,
    /// The release keystore, project-relative (or absolute). A store build
    /// signs with it; a device build uses the debug key regardless. Its
    /// passwords are never stored here - they come from the environment
    /// (`JENGINE_KEYSTORE_PASSWORD`) or the editor's Build dialog.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keystore: String,
    /// The key's alias inside the keystore.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key_alias: String,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl AppSettings {
    /// Nothing authored - what `skip_serializing_if` checks, so an editor
    /// saving an untouched project writes no `"app": {}`.
    pub fn is_unset(&self) -> bool {
        *self == Self::default()
    }
}

impl AndroidSettings {
    /// Nothing authored; see [`AppSettings::is_unset`].
    pub fn is_unset(&self) -> bool {
        *self == Self::default()
    }
}

/// Android 8.0: AAudio (what cpal plays through) and Vulkan are both there.
pub const DEFAULT_MIN_SDK: u32 = 26;
/// Google Play refuses uploads that target less than the previous year's
/// Android; 36 is Android 16 (2025).
pub const DEFAULT_TARGET_SDK: u32 = 36;
pub const DEFAULT_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Landscape,
    Portrait,
    Any,
}

/// Settings with every default applied - what a build uses.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedApp {
    pub id: String,
    /// True when `id` was made up from the project name. Fine for a device
    /// test; a store build must name its own.
    pub id_defaulted: bool,
    pub name: String,
    pub version: String,
    pub build: u32,
    /// Project-relative, or `None` for the platform's default icon.
    pub icon: Option<String>,
    /// `[r, g, b]`.
    pub icon_background: [u8; 3],
    pub orientation: Orientation,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub abis: Vec<String>,
    pub immersive: bool,
    /// The release keystore as authored (project-relative or absolute), if any.
    pub keystore: Option<String>,
    pub key_alias: String,
}

/// The parts of `project.jengine` resolution reads.
#[derive(Debug, Default, Deserialize)]
struct ProjectDoc {
    #[serde(default)]
    name: String,
    #[serde(default)]
    settings: SettingsDoc,
}

#[derive(Debug, Default, Deserialize)]
struct SettingsDoc {
    #[serde(default)]
    title: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    app: AppSettings,
    #[serde(default)]
    android: AndroidSettings,
}

/// Read and resolve a project's app settings.
pub fn read(project_root: &Path) -> Result<ResolvedApp> {
    let path = project_root.join("project.jengine");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let doc: ProjectDoc = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
    let folder = project_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("game");
    let project_name = if doc.name.is_empty() { folder } else { &doc.name };
    let s = doc.settings;
    s.app.resolve(&s.android, project_name, &s.title, (s.width, s.height))
}

impl AppSettings {
    /// Fill every unset field and validate the result.
    ///
    /// `title` and `size` are the project's own `settings.title` and
    /// `width` x `height`, which the name and orientation default from.
    pub fn resolve(
        &self,
        android: &AndroidSettings,
        project_name: &str,
        title: &str,
        size: (u32, u32),
    ) -> Result<ResolvedApp> {
        let (id, id_defaulted) = if self.id.trim().is_empty() {
            (format!("com.jengine.{}", id_segment(project_name)), true)
        } else {
            (self.id.trim().to_string(), false)
        };
        validate_id(&id)?;

        let name = [self.name.trim(), title.trim(), project_name.trim()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("jEngine Game")
            .to_string();

        let orientation = match self.orientation.trim().to_ascii_lowercase().as_str() {
            "" => {
                if size.1 > size.0 {
                    Orientation::Portrait
                } else {
                    Orientation::Landscape
                }
            }
            "landscape" => Orientation::Landscape,
            "portrait" => Orientation::Portrait,
            "any" | "sensor" | "both" => Orientation::Any,
            other => bail!("settings.app.orientation '{other}' is not landscape, portrait or any"),
        };

        let icon_background = if self.icon_background.trim().is_empty() {
            [0xff, 0xff, 0xff]
        } else {
            parse_hex_colour(&self.icon_background).ok_or_else(|| {
                anyhow::anyhow!(
                    "settings.app.icon_background '{}' is not #rrggbb",
                    self.icon_background
                )
            })?
        };

        let min_sdk = if android.min_sdk == 0 { DEFAULT_MIN_SDK } else { android.min_sdk };
        let target_sdk = if android.target_sdk == 0 { DEFAULT_TARGET_SDK } else { android.target_sdk };
        if target_sdk < min_sdk {
            bail!("settings.android.target_sdk {target_sdk} is below min_sdk {min_sdk}");
        }
        let abis = if android.abis.is_empty() {
            vec!["arm64-v8a".to_string()]
        } else {
            android.abis.clone()
        };
        for abi in &abis {
            if !matches!(abi.as_str(), "arm64-v8a" | "x86_64" | "armeabi-v7a" | "x86") {
                bail!("settings.android.abis: unknown ABI '{abi}'");
            }
        }

        Ok(ResolvedApp {
            id,
            id_defaulted,
            name,
            version: if self.version.trim().is_empty() {
                DEFAULT_VERSION.to_string()
            } else {
                self.version.trim().to_string()
            },
            build: self.build.max(1),
            icon: Some(self.icon.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            icon_background,
            orientation,
            min_sdk,
            target_sdk,
            abis,
            immersive: android.immersive.unwrap_or(true),
            keystore: Some(android.keystore.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            key_alias: android.key_alias.trim().to_string(),
        })
    }
}

/// Android's package-name rules, which are also safe as an iOS bundle id:
/// two or more dot-separated segments, each a letter followed by letters,
/// digits or underscores. Refused here rather than by `aapt2`, whose message
/// does not say which setting is wrong.
pub fn validate_id(id: &str) -> Result<()> {
    let segments: Vec<&str> = id.split('.').collect();
    if segments.len() < 2 {
        bail!("app id '{id}' needs at least two dot-separated parts, like com.example.game");
    }
    for seg in segments {
        let mut chars = seg.chars();
        let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            bail!(
                "app id '{id}': '{seg}' must start with a letter and hold only letters, digits and _"
            );
        }
    }
    Ok(())
}

/// A project name as one package-name segment: `example-arcade` becomes
/// `example_arcade`, `2048` becomes `g2048`.
fn id_segment(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_alphabetic()) {
        out.insert(0, 'g');
    }
    out
}

fn parse_hex_colour(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(app: AppSettings) -> Result<ResolvedApp> {
        app.resolve(&AndroidSettings::default(), "example-arcade", "Example Arcade", (1280, 720))
    }

    #[test]
    fn an_empty_block_resolves_to_usable_defaults() {
        let r = resolve(AppSettings::default()).unwrap();
        assert_eq!(r.id, "com.jengine.example_arcade");
        assert!(r.id_defaulted);
        assert_eq!(r.name, "Example Arcade");
        assert_eq!(r.version, "1.0.0");
        assert_eq!(r.build, 1);
        assert_eq!(r.icon, None);
        assert_eq!(r.orientation, Orientation::Landscape);
        assert_eq!((r.min_sdk, r.target_sdk), (DEFAULT_MIN_SDK, DEFAULT_TARGET_SDK));
        assert_eq!(r.abis, vec!["arm64-v8a"]);
        assert!(r.immersive, "a game hides the system bars unless told not to");
    }

    #[test]
    fn a_tall_project_defaults_to_portrait() {
        let r = AppSettings::default()
            .resolve(&AndroidSettings::default(), "g", "", (720, 1280))
            .unwrap();
        assert_eq!(r.orientation, Orientation::Portrait);
    }

    #[test]
    fn authored_values_win() {
        let r = resolve(AppSettings {
            id: "com.example.arcade".into(),
            name: "Arcade".into(),
            version: "2.1".into(),
            build: 9,
            icon: "icon.png".into(),
            icon_background: "#10141c".into(),
            orientation: "any".into(),
        })
        .unwrap();
        assert_eq!(r.id, "com.example.arcade");
        assert!(!r.id_defaulted);
        assert_eq!((r.name.as_str(), r.version.as_str(), r.build), ("Arcade", "2.1", 9));
        assert_eq!(r.icon.as_deref(), Some("icon.png"));
        assert_eq!(r.icon_background, [0x10, 0x14, 0x1c]);
        assert_eq!(r.orientation, Orientation::Any);
    }

    #[test]
    fn bad_ids_are_refused_with_the_reason() {
        for bad in ["game", "com.example.2048", "com..game", "com.my-game", "com.game."] {
            let err = resolve(AppSettings { id: bad.into(), ..Default::default() })
                .unwrap_err()
                .to_string();
            assert!(err.contains(bad), "{bad}: {err}");
        }
    }

    #[test]
    fn a_project_name_that_is_not_an_identifier_still_makes_an_id() {
        let r = AppSettings::default()
            .resolve(&AndroidSettings::default(), "2048 deluxe!", "", (1, 1))
            .unwrap();
        assert_eq!(r.id, "com.jengine.g2048_deluxe_");
    }

    #[test]
    fn unset_fields_are_not_written_back() {
        // An editor saving an untouched project must not fill its file with
        // defaults, or changing a default later would not reach it.
        let json = serde_json::to_value(AppSettings::default()).unwrap();
        assert_eq!(json, serde_json::json!({}));
        let json = serde_json::to_value(AndroidSettings::default()).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }
}
