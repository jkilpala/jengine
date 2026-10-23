//! Package manifest types — how projects declare package dependencies
//! and how packages describe themselves.

use serde::{Deserialize, Serialize};

// ──────────────────────────── Package Source ────────────────────────────

/// Where a package is fetched from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PackageSource {
    /// Git repository — cloned to `.jengine/packages/<name>-<hash>/`.
    Git {
        url: String,
        /// Git tag to check out (e.g. "v0.1.0").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tag: Option<String>,
        /// Exact commit hash to check out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rev: Option<String>,
    },
    /// Local path (relative to project root or absolute).
    Path { path: String },
}

// ──────────────────────────── Package Dependency ────────────────────────────

/// A package dependency declared in `project.jengine` under the `"packages"` array.
///
/// ```json
/// {
///   "packages": [
///     { "name": "jengine-physics", "source": { "type": "path", "path": "../packages/jengine-physics" } },
///     { "name": "my-plugin", "source": { "type": "git", "url": "https://github.com/user/my-plugin.git", "tag": "v0.1.0" } }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageDependency {
    /// Package name (must match the crate name, e.g. "jengine-physics").
    pub name: String,
    /// Where to fetch the package from.
    pub source: PackageSource,
    /// If true, the package is optional and can be toggled off without removing it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

// ──────────────────────────── Package Manifest (package.toml) ────────────────────────────

/// Manifest file shipped inside a package crate (`package.toml`).
/// Describes the package's identity, compatibility, and dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageManifestInfo,
    /// Script file patterns for script packages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<ScriptSection>,
    /// Asset file patterns for asset packages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<AssetSection>,
}

// ──────────────────────────── Package Type ────────────────────────────

/// The kind of package — determines how it's resolved and installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    /// Full Rust crate with `Cargo.toml` and `JEnginePackage` implementation.
    #[default]
    Full,
    /// Script-only package — Lua/JS files copied to project assets.
    Script,
    /// Asset-only package — sprites, sounds, prefabs copied to project assets.
    Asset,
}

// ──────────────────────────── Script Section ────────────────────────────

/// The `[scripts]` section of a `package.toml` for script packages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptSection {
    /// Glob patterns for script files relative to the package root
    /// (e.g. `["scripts/**/*.lua", "scripts/**/*.js"]`).
    pub files: Vec<String>,
}

// ──────────────────────────── Asset Section ────────────────────────────

/// The `[assets]` section of a `package.toml` for asset packages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSection {
    /// Glob patterns for asset files relative to the package root
    /// (e.g. `["sprites/**/*.png", "sounds/**/*.ogg"]`).
    pub files: Vec<String>,
    /// If true, prefab files (`.prefab`) in the package are also installed.
    #[serde(default)]
    pub include_prefabs: bool,
}

/// The `[package]` section of a `package.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifestInfo {
    /// Unique package name (must match Cargo crate name for full packages).
    pub name: String,
    /// Semver version string (e.g. "0.1.0").
    pub version: String,
    /// Short description of what the package provides.
    #[serde(default)]
    pub description: String,
    /// Author name or organization.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    /// Minimum compatible jengine-core version (semver).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    /// Names of other packages this package depends on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Package type: "full" (default), "script", or "asset".
    #[serde(default)]
    pub package_type: PackageType,
    /// Rust path to the type implementing `JEnginePackage`, e.g.
    /// `"my_package::MyPackage"`. Full packages only.
    ///
    /// Used by the generated per-project runtime to write the
    /// `host.add_package(Box::new(...))` line, so it has to name a type that
    /// is `pub` from the crate root. Omit it and
    /// [`PackageManifestInfo::derived_entry_point`] guesses from the crate
    /// name; declare it whenever the guess would be wrong, which is most of
    /// the time for a name that is not `<something>-package`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<String>,
}

impl PackageManifestInfo {
    /// The `JEnginePackage` type to construct, explicit if declared.
    ///
    /// The fallback mirrors how the built-ins are named: drop a leading
    /// `jengine-`, upper-camel the rest, append `Package` — `jengine-parallax`
    /// becomes `jengine_parallax::ParallaxPackage`. It is a guess, and a
    /// generated runtime that fails to compile because of it is telling the
    /// author to set `entry_point` explicitly.
    pub fn resolved_entry_point(&self) -> String {
        if let Some(explicit) = &self.entry_point {
            return explicit.clone();
        }
        self.derived_entry_point()
    }

    /// The guessed entry point, ignoring any explicit declaration.
    pub fn derived_entry_point(&self) -> String {
        let module = self.name.replace('-', "_");
        let stem = self.name.strip_prefix("jengine-").unwrap_or(&self.name);
        let mut camel = String::new();
        for part in stem.split(['-', '_']).filter(|p| !p.is_empty()) {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                camel.extend(first.to_uppercase());
                camel.push_str(chars.as_str());
            }
        }
        format!("{module}::{camel}Package")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, entry: Option<&str>) -> PackageManifestInfo {
        PackageManifestInfo {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            author: String::new(),
            engine_version: None,
            dependencies: vec![],
            package_type: PackageType::Full,
            entry_point: entry.map(str::to_string),
        }
    }

    #[test]
    fn entry_point_is_derived_the_way_the_built_ins_are_named() {
        assert_eq!(
            info("jengine-parallax", None).resolved_entry_point(),
            "jengine_parallax::ParallaxPackage"
        );
        // A third-party name keeps its whole stem.
        assert_eq!(
            info("cool-fx", None).resolved_entry_point(),
            "cool_fx::CoolFxPackage"
        );
    }

    #[test]
    fn an_explicit_entry_point_wins() {
        assert_eq!(
            info("cool-fx", Some("cool_fx::Renderer")).resolved_entry_point(),
            "cool_fx::Renderer"
        );
    }

    #[test]
    fn package_dependency_path_roundtrip() {
        let dep = PackageDependency {
            name: "jengine-physics".to_string(),
            source: PackageSource::Path {
                path: "../packages/jengine-physics".to_string(),
            },
            optional: false,
        };
        let json = serde_json::to_string(&dep).unwrap();
        let back: PackageDependency = serde_json::from_str(&json).unwrap();
        assert_eq!(dep, back);
        assert!(!json.contains("optional")); // skipped when false
    }

    #[test]
    fn package_dependency_git_roundtrip() {
        let dep = PackageDependency {
            name: "my-plugin".to_string(),
            source: PackageSource::Git {
                url: "https://github.com/user/my-plugin.git".to_string(),
                tag: Some("v0.1.0".to_string()),
                rev: None,
            },
            optional: true,
        };
        let json = serde_json::to_string_pretty(&dep).unwrap();
        let back: PackageDependency = serde_json::from_str(&json).unwrap();
        assert_eq!(dep, back);
        assert!(json.contains("\"optional\": true"));
    }

    #[test]
    fn package_manifest_toml_roundtrip() {
        let manifest = PackageManifest {
            package: PackageManifestInfo {
                name: "jengine-physics".to_string(),
                version: "0.1.0".to_string(),
                description: "Physics package for jEngine".to_string(),
                author: "jEngine Team".to_string(),
                engine_version: Some("0.1.0".to_string()),
                dependencies: vec![],
                package_type: PackageType::Full,
                entry_point: None,
            },
            scripts: None,
            assets: None,
        };
        let toml_str = toml::to_string_pretty(&manifest).unwrap();
        let back: PackageManifest = toml::from_str(&toml_str).unwrap();
        assert_eq!(manifest.package.name, back.package.name);
        assert_eq!(manifest.package.version, back.package.version);
        assert_eq!(back.package.package_type, PackageType::Full);
        assert!(back.scripts.is_none());
        assert!(back.assets.is_none());
    }

    #[test]
    fn script_package_manifest_roundtrip() {
        let manifest = PackageManifest {
            package: PackageManifestInfo {
                name: "my-scripts".to_string(),
                version: "1.0.0".to_string(),
                description: "Script pack".to_string(),
                author: String::new(),
                engine_version: None,
                dependencies: vec![],
                package_type: PackageType::Script,
                entry_point: None,
            },
            scripts: Some(ScriptSection {
                files: vec!["scripts/**/*.lua".to_string(), "scripts/**/*.js".to_string()],
            }),
            assets: None,
        };
        let toml_str = toml::to_string_pretty(&manifest).unwrap();
        let back: PackageManifest = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.package.package_type, PackageType::Script);
        let scripts = back.scripts.unwrap();
        assert_eq!(scripts.files.len(), 2);
        assert!(scripts.files[0].contains("lua"));
    }

    #[test]
    fn asset_package_manifest_roundtrip() {
        let manifest = PackageManifest {
            package: PackageManifestInfo {
                name: "sprite-pack".to_string(),
                version: "0.2.0".to_string(),
                description: "Sprites".to_string(),
                author: String::new(),
                engine_version: None,
                dependencies: vec![],
                package_type: PackageType::Asset,
                entry_point: None,
            },
            scripts: None,
            assets: Some(AssetSection {
                files: vec!["sprites/**/*.png".to_string()],
                include_prefabs: true,
            }),
        };
        let toml_str = toml::to_string_pretty(&manifest).unwrap();
        let back: PackageManifest = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.package.package_type, PackageType::Asset);
        let assets = back.assets.unwrap();
        assert_eq!(assets.files.len(), 1);
        assert!(assets.include_prefabs);
    }

    #[test]
    fn package_type_default_is_full() {
        let toml_str = r#"
[package]
name = "old-style"
version = "1.0.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.package.package_type, PackageType::Full);
    }

    #[test]
    fn packages_array_in_project_json() {
        let json = r#"{
            "name": "TestProject",
            "packages": [
                {
                    "name": "jengine-physics",
                    "source": { "type": "path", "path": "packages/jengine-physics" }
                },
                {
                    "name": "cool-plugin",
                    "source": { "type": "git", "url": "https://example.com/cool.git", "tag": "v1.0" },
                    "optional": true
                }
            ]
        }"#;

        // Parse just the packages array
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let packages: Vec<PackageDependency> =
            serde_json::from_value(value["packages"].clone()).unwrap();

        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "jengine-physics");
        assert!(!packages[0].optional);
        assert!(matches!(packages[0].source, PackageSource::Path { .. }));
        assert_eq!(packages[1].name, "cool-plugin");
        assert!(packages[1].optional);
        assert!(matches!(
            packages[1].source,
            PackageSource::Git { ref tag, .. } if tag.as_deref() == Some("v1.0")
        ));
    }
}
