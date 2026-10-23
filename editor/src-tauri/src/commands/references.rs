//! Asset references: what names an asset, and keeping it named when the
//! asset moves.
//!
//! Every reference in a project is a path string - a sprite's `texture_id`,
//! a scene's `audio_refs`, a script's `"sounds/hit.wav"`, a project's default
//! scene - written relative to `assets/` (or, in a few places, with the
//! `assets/` in front). So renaming a file in the asset browser used to break
//! every scene that drew it, silently: the file moved, the strings did not.
//!
//! A reference is the path as a **whole quoted string** (`"sprites/a.png"`,
//! `'sprites/a.png'`), or for a folder the quoted start of one
//! (`"sprites/enemies/...`) - so `a.png` never matches inside `ba.png`, and a
//! path a script builds by concatenation is not found (nothing could find
//! that). The rewrite is the same rule, used for files on disk and, through
//! `rewrite_asset_references_in_text`, for the scene the editor has open.

use serde::Serialize;
use std::path::{Path, PathBuf};

use super::search::{BINARY_EXTS, MAX_FILE_BYTES, SKIP_DIRS};

/// `assets/sprites/a.png` -> `sprites/a.png`: the form references use.
fn asset_relative(project_relative: &str) -> &str {
    let p = project_relative.trim_start_matches("./");
    p.strip_prefix("assets/").unwrap_or(p)
}

/// The quoted forms a reference to `asset` (assets-relative) takes. For a
/// folder, the forms open a path inside it.
fn reference_forms(asset: &str, folder: bool) -> Vec<String> {
    let mut forms = Vec::new();
    for q in ['"', '\''] {
        for prefix in ["", "assets/"] {
            if folder {
                forms.push(format!("{q}{prefix}{asset}/"));
            } else {
                forms.push(format!("{q}{prefix}{asset}{q}"));
            }
        }
    }
    forms
}

/// `content` with every reference to `old` renamed to `new` (both
/// assets-relative), and how many it renamed.
pub fn rewrite_references(content: &str, old: &str, new: &str, folder: bool) -> (String, usize) {
    let mut out = content.to_string();
    let mut count = 0;
    for (from, to) in reference_forms(old, folder).into_iter().zip(reference_forms(new, folder)) {
        let n = out.matches(&from).count();
        if n > 0 {
            count += n;
            out = out.replace(&from, &to);
        }
    }
    (out, count)
}

/// One line naming the asset.
#[derive(Serialize, Debug, PartialEq)]
pub struct ReferenceHit {
    /// Project-relative, forward slashes.
    pub path: String,
    /// 1-based.
    pub line: u32,
    pub text: String,
}

fn reference_lines(rel: &str, content: &str, forms: &[String], out: &mut Vec<ReferenceHit>) {
    for (i, line) in content.lines().enumerate() {
        if forms.iter().any(|f| line.contains(f.as_str())) {
            let text: String = line.trim().chars().take(200).collect();
            out.push(ReferenceHit { path: rel.to_string(), line: i as u32 + 1, text });
        }
    }
}

/// Every text file under `dir`, project-relative, sorted.
fn text_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                text_files(root, &path, out);
            }
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        if ext.as_deref().is_some_and(|e| BINARY_EXTS.contains(&e)) {
            continue;
        }
        if path.metadata().map(|m| m.len() > MAX_FILE_BYTES).unwrap_or(true) {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            out.push((rel.to_string_lossy().replace('\\', "/"), path));
        }
    }
}

/// Where the project names the asset at `relative_path` (project-relative,
/// a file or a folder): every line, in every text file but the asset itself.
#[tauri::command(async)]
pub fn find_asset_references(project_path: String, relative_path: String) -> Result<Vec<ReferenceHit>, String> {
    if relative_path.contains("..") {
        return Err("Invalid path".into());
    }
    let root = PathBuf::from(&project_path);
    let folder = root.join(&relative_path).is_dir();
    let forms = reference_forms(asset_relative(&relative_path), folder);
    let mut files = Vec::new();
    text_files(&root, &root, &mut files);
    let mut hits = Vec::new();
    for (rel, path) in files {
        if rel == relative_path || (folder && rel.starts_with(&format!("{relative_path}/"))) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            reference_lines(&rel, &content, &forms, &mut hits);
        }
    }
    Ok(hits)
}

#[derive(Serialize, Debug)]
pub struct RenameSummary {
    /// Files whose references were rewritten.
    pub files_changed: Vec<String>,
    pub references: usize,
    /// Files that named the asset but could not be rewritten.
    pub failures: Vec<String>,
}

/// Rename (or move) an asset and every reference to it. The asset moves
/// first; a file that then fails to be rewritten is reported, not undone -
/// the move itself is what was asked for.
#[tauri::command(async)]
pub fn rename_asset_with_references(
    project_path: String,
    old_relative_path: String,
    new_relative_path: String,
) -> Result<RenameSummary, String> {
    let root = PathBuf::from(&project_path);
    let folder = root.join(&old_relative_path).is_dir();
    super::project::rename_asset(project_path.clone(), old_relative_path.clone(), new_relative_path.clone())?;
    let old = asset_relative(&old_relative_path).to_string();
    let new = asset_relative(&new_relative_path).to_string();
    let mut files = Vec::new();
    text_files(&root, &root, &mut files);
    let mut summary = RenameSummary { files_changed: Vec::new(), references: 0, failures: Vec::new() };
    for (rel, path) in files {
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let (rewritten, n) = rewrite_references(&content, &old, &new, folder);
        if n == 0 {
            continue;
        }
        match std::fs::write(&path, rewritten) {
            Ok(()) => {
                summary.files_changed.push(rel);
                summary.references += n;
            }
            Err(e) => summary.failures.push(format!("{rel}: {e}")),
        }
    }
    Ok(summary)
}

/// The same rewrite on text the editor holds - the open scene - so what is in
/// memory agrees with what was written to disk. Paths project-relative, as
/// the asset browser has them.
#[tauri::command]
pub fn rewrite_asset_references_in_text(
    text: String,
    old_relative_path: String,
    new_relative_path: String,
    folder: bool,
) -> (String, usize) {
    rewrite_references(&text, asset_relative(&old_relative_path), asset_relative(&new_relative_path), folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_whole_quoted_paths_only() {
        let scene = r#"{"texture_id": "sprites/a.png", "other": "sprites/ba.png", "x": "sprites/a.png.bak", "y": "assets/sprites/a.png"}"#;
        let (out, n) = rewrite_references(scene, "sprites/a.png", "art/hero.png", false);
        assert_eq!(n, 2);
        assert_eq!(
            out,
            r#"{"texture_id": "art/hero.png", "other": "sprites/ba.png", "x": "sprites/a.png.bak", "y": "assets/art/hero.png"}"#
        );
        let (lua, n) = rewrite_references("Audio.play_sound('sounds/hit.wav', 1)", "sounds/hit.wav", "sfx/hit.wav", false);
        assert_eq!((lua.as_str(), n), ("Audio.play_sound('sfx/hit.wav', 1)", 1));
    }

    #[test]
    fn renames_what_is_inside_a_folder() {
        let (out, n) = rewrite_references(
            r#"["sprites/enemies/bat.png", "sprites/enemies2/x.png", "sprites/enemies"]"#,
            "sprites/enemies",
            "sprites/foes",
            true,
        );
        assert_eq!(n, 1);
        assert_eq!(out, r#"["sprites/foes/bat.png", "sprites/enemies2/x.png", "sprites/enemies"]"#);
    }

    #[test]
    fn finds_and_rewrites_on_disk() {
        let dir = std::env::temp_dir().join(format!("jengine_refs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets/sprites")).unwrap();
        std::fs::create_dir_all(dir.join("assets/scenes")).unwrap();
        std::fs::write(dir.join("assets/sprites/a.png"), [0u8, 1, 2]).unwrap();
        std::fs::write(dir.join("assets/scenes/one.scene"), "{\n  \"texture_id\": \"sprites/a.png\"\n}\n").unwrap();
        std::fs::write(dir.join("project.jengine"), "{\"icon\": \"assets/sprites/a.png\"}").unwrap();
        let project = dir.to_string_lossy().to_string();

        let hits = find_asset_references(project.clone(), "assets/sprites/a.png".into()).unwrap();
        assert_eq!(
            hits.iter().map(|h| (h.path.as_str(), h.line)).collect::<Vec<_>>(),
            vec![("assets/scenes/one.scene", 2), ("project.jengine", 1)]
        );

        let summary =
            rename_asset_with_references(project.clone(), "assets/sprites/a.png".into(), "assets/sprites/hero.png".into())
                .unwrap();
        assert_eq!(summary.references, 2);
        assert!(dir.join("assets/sprites/hero.png").exists());
        let scene = std::fs::read_to_string(dir.join("assets/scenes/one.scene")).unwrap();
        assert!(scene.contains("\"sprites/hero.png\""), "{scene}");
        assert!(find_asset_references(project, "assets/sprites/hero.png".into()).unwrap().len() == 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
