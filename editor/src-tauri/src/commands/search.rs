//! Project-wide text search — the editor's "find in files".
//!
//! Walks the project once per query and returns matching lines. There is no
//! index: projects here are a few thousand small text files, and a cold walk
//! costs less than keeping an index honest across external edits.
//!
//! Everything about this is bounded on purpose. A search for `e` in a large
//! project would otherwise return a million hits, serialize them all across
//! the IPC boundary, and hang the editor rendering them — so hits, file size
//! and line length all have caps, and the caller is told when a cap was hit
//! rather than being handed a silently short list.

use serde::Serialize;
use std::path::Path;

/// Maximum hits returned. Past this the result is marked `truncated` and the
/// walk stops — a query this broad needs refining, not more scrolling.
const MAX_HITS: usize = 5_000;

/// Files larger than this are skipped. Generated data tables and vendored
/// bundles blow past it; hand-written source does not.
pub(crate) const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Matched lines are clipped to this many characters for display. A minified
/// bundle on one 400 KB line would otherwise be shipped whole, per hit.
const MAX_LINE_CHARS: usize = 400;

/// Directories never worth searching: build output, dependencies, VCS, and
/// the engine's own generated crates.
pub(crate) const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    ".jengine",
    "dist",
    ".svelte-kit",
];

/// Extensions read as bytes, not text. Checked before opening the file so a
/// 40 MB texture is never read just to discover it is not UTF-8.
pub(crate) const BINARY_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "ico", "tga", "psd", "ttf", "otf", "woff", "woff2",
    "wav", "ogg", "mp3", "flac", "aac", "mp4", "webm", "mov", "zip", "gz", "7z", "rar", "exe",
    "dll", "so", "dylib", "pdb", "lib", "rlib", "wasm", "bin", "pdf", "ase", "aseprite",
];

/// One matching line.
#[derive(Serialize, Debug)]
pub struct SearchHit {
    /// Project-root-relative with forward slashes — the same shape
    /// `AssetNode.path` uses, so a result can be handed straight to the
    /// editor's open-by-type dispatch without rewriting.
    pub path: String,
    /// 1-based, matching what editors display and what the script editor's
    /// line-jump expects.
    pub line: u32,
    /// Character offsets `[start, end)` of every match on this line, for
    /// highlighting. Characters rather than bytes so the UI can slice the
    /// string it was given without decoding UTF-8 itself.
    pub matches: Vec<(u32, u32)>,
    /// The line, trailing whitespace removed and clipped to `MAX_LINE_CHARS`.
    pub text: String,
    /// The line was longer than the clip limit.
    pub clipped: bool,
}

/// What a search found, and whether it had to stop early.
#[derive(Serialize)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    /// Distinct files containing at least one hit.
    pub files_matched: usize,
    /// Files actually read and scanned.
    pub files_searched: usize,
    /// `MAX_HITS` was reached and the walk stopped.
    pub truncated: bool,
}

/// Search every text file under `project_path`.
///
/// `async` is load-bearing, the same way it is for the build commands: Tauri
/// runs a *synchronous* command on the main thread, which on Windows is the
/// thread pumping WebView2 messages. Walking a few thousand files there would
/// freeze the editor for the length of every keystroke-triggered search.
#[tauri::command(async)]
pub fn search_in_files(
    project_path: String,
    query: String,
    case_sensitive: bool,
    whole_word: bool,
    is_regex: bool,
) -> Result<SearchResults, String> {
    let root = std::path::PathBuf::from(&project_path);
    if !root.is_dir() {
        return Err(format!("Project not found: {project_path}"));
    }
    let matcher = build_matcher(&query, case_sensitive, whole_word, is_regex)?;
    Ok(run_search(&root, &matcher))
}

/// Compile the query into one regex, whatever mode was asked for.
///
/// Literal mode escapes the query rather than taking a separate code path, so
/// "find in files" behaves identically apart from what the pattern means —
/// one matcher, one set of edge cases.
fn build_matcher(
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    is_regex: bool,
) -> Result<regex::Regex, String> {
    if query.is_empty() {
        return Err("Search query is empty".to_string());
    }
    let body = if is_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    // Non-capturing group: a bare alternation like `a|b` would otherwise bind
    // the word boundaries to only the first and last branch.
    let pattern = if whole_word {
        format!(r"\b(?:{body})\b")
    } else {
        body
    };
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        // Search matches line by line, so `^` and `$` already mean line
        // boundaries there. Replace runs against whole file contents, and
        // without this a pattern like `^local` would match only the first line
        // of the file - finding twenty hits and replacing one.
        .multi_line(true)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| format!("Invalid pattern: {e}"))
}

fn run_search(root: &Path, matcher: &regex::Regex) -> SearchResults {
    let mut results = SearchResults {
        hits: Vec::new(),
        files_matched: 0,
        files_searched: 0,
        truncated: false,
    };
    walk(root, root, matcher, &mut results);
    results
}

fn walk(root: &Path, dir: &Path, matcher: &regex::Regex, out: &mut SearchResults) {
    if out.truncated {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    // Sorted so results are stable between runs; an unordered walk makes a
    // result list reshuffle itself when nothing changed.
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if out.truncated {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with('.') {
                walk(root, &path, matcher, out);
            }
        } else {
            search_file(root, &path, matcher, out);
        }
    }
}

fn search_file(root: &Path, path: &Path, matcher: &regex::Regex, out: &mut SearchResults) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if let Some(ext) = &ext
        && BINARY_EXTS.contains(&ext.as_str())
    {
        return;
    }
    match path.metadata() {
        Ok(m) if m.len() > MAX_FILE_BYTES => return,
        Ok(_) => {}
        Err(_) => return,
    }
    // Anything that is not UTF-8 is not something this editor can show, and
    // `read_to_string` failing is the cheapest reliable binary check there is.
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    out.files_searched += 1;

    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    let mut file_had_hit = false;
    for (idx, line) in content.lines().enumerate() {
        if out.hits.len() >= MAX_HITS {
            out.truncated = true;
            break;
        }
        // Byte offsets from the regex, converted to character offsets so the
        // frontend can slice the JS string it receives. They differ the moment
        // a line contains any non-ASCII, and slicing by byte offset there
        // splits characters.
        let matches: Vec<(u32, u32)> = matcher
            .find_iter(line)
            .map(|m| {
                (
                    line[..m.start()].chars().count() as u32,
                    line[..m.end()].chars().count() as u32,
                )
            })
            .collect();
        if matches.is_empty() {
            continue;
        }
        file_had_hit = true;

        let trimmed = line.trim_end();
        let clipped = trimmed.chars().count() > MAX_LINE_CHARS;
        let text = if clipped {
            trimmed.chars().take(MAX_LINE_CHARS).collect()
        } else {
            trimmed.to_string()
        };
        out.hits.push(SearchHit {
            path: rel.clone(),
            line: idx as u32 + 1,
            matches,
            text,
            clipped,
        });
    }
    if file_had_hit {
        out.files_matched += 1;
    }
}

// ---------------------------------------------------------------------------
// Replace
// ---------------------------------------------------------------------------

/// One line that would change, shown before anything is written.
#[derive(Serialize, Debug)]
pub struct ReplacePreviewHit {
    pub path: String,
    pub line: u32,
    pub before: String,
    pub after: String,
}

/// What a replace would do. Nothing is written to produce this.
#[derive(Serialize, Debug)]
pub struct ReplacePreview {
    pub hits: Vec<ReplacePreviewHit>,
    /// Distinct files that would change, in walk order.
    pub files: Vec<String>,
    pub truncated: bool,
}

/// What a replace actually did.
#[derive(Serialize, Debug)]
pub struct ReplaceSummary {
    pub files_changed: usize,
    pub replacements: usize,
    /// Files that were asked for but could not be written, with the reason.
    pub failures: Vec<String>,
}

/// Turn the caller's replacement text into what `regex` expects.
///
/// In regex mode `$1` and `${name}` are capture references, which is the point
/// of the mode. In literal mode they are not: someone replacing a string with
/// `$PATH` means those five characters, so `$` is escaped.
fn expand_replacement(replacement: &str, is_regex: bool) -> String {
    if is_regex {
        replacement.to_string()
    } else {
        replacement.replace('$', "$$")
    }
}

/// Lines that differ once `matcher` is replaced, as `(1-based line, before, after)`.
fn changed_lines(
    content: &str,
    matcher: &regex::Regex,
    replacement: &str,
) -> Vec<(u32, String, String)> {
    let replaced = matcher.replace_all(content, replacement);
    let mut out = Vec::new();
    for (idx, (before, after)) in content.lines().zip(replaced.lines()).enumerate() {
        if before != after {
            out.push((idx as u32 + 1, before.to_string(), after.to_string()));
        }
    }
    out
}

/// Show what a replace would change, touching nothing.
///
/// The preview is not politeness: a bulk edit across a project has no undo and
/// the editor cannot offer one. Seeing the diff first is the only safety this
/// operation gets.
#[tauri::command(async)]
pub fn preview_replace_in_files(
    project_path: String,
    query: String,
    replacement: String,
    case_sensitive: bool,
    whole_word: bool,
    is_regex: bool,
) -> Result<ReplacePreview, String> {
    let root = std::path::PathBuf::from(&project_path);
    if !root.is_dir() {
        return Err(format!("Project not found: {project_path}"));
    }
    let matcher = build_matcher(&query, case_sensitive, whole_word, is_regex)?;
    let replacement = expand_replacement(&replacement, is_regex);

    // Reuse exactly the walk the search panel used, so a preview can never
    // cover a different set of files than the search that produced it.
    let found = run_search(&root, &matcher);

    let mut files: Vec<String> = Vec::new();
    for hit in &found.hits {
        if !files.iter().any(|f| f == &hit.path) {
            files.push(hit.path.clone());
        }
    }

    let mut hits = Vec::new();
    for rel in &files {
        let Ok(content) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (line, before, after) in changed_lines(&content, &matcher, &replacement) {
            hits.push(ReplacePreviewHit {
                path: rel.clone(),
                line,
                before,
                after,
            });
        }
    }

    Ok(ReplacePreview {
        hits,
        files,
        truncated: found.truncated,
    })
}

/// Apply the replacement to `paths` only.
///
/// Takes an explicit file list rather than re-deciding what to touch: the user
/// approved a preview naming specific files, and anything created or edited
/// since then must not be swept up by the same click.
///
/// Whole files are the unit. Excluding individual lines would mean
/// reconstructing content from the preview, which stops being correct the
/// moment the file changes underneath it.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn apply_replace_in_files(
    project_path: String,
    query: String,
    replacement: String,
    case_sensitive: bool,
    whole_word: bool,
    is_regex: bool,
    paths: Vec<String>,
) -> Result<ReplaceSummary, String> {
    let root = std::path::PathBuf::from(&project_path);
    if !root.is_dir() {
        return Err(format!("Project not found: {project_path}"));
    }
    let matcher = build_matcher(&query, case_sensitive, whole_word, is_regex)?;
    let replacement = expand_replacement(&replacement, is_regex);

    let mut summary = ReplaceSummary {
        files_changed: 0,
        replacements: 0,
        failures: Vec::new(),
    };

    for rel in paths {
        // `paths` arrives from the frontend. A `..` segment would let a
        // replace rewrite files outside the project entirely.
        if rel.contains("..") {
            summary
                .failures
                .push(format!("{rel}: path escapes the project"));
            continue;
        }
        let full = root.join(&rel);
        let content = match std::fs::read_to_string(&full) {
            Ok(c) => c,
            Err(e) => {
                summary.failures.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let count = matcher.find_iter(&content).count();
        if count == 0 {
            continue;
        }
        let replaced = matcher.replace_all(&content, replacement.as_str());
        match std::fs::write(&full, replaced.as_ref()) {
            Ok(()) => {
                summary.files_changed += 1;
                summary.replacements += count;
            }
            Err(e) => summary.failures.push(format!("{rel}: {e}")),
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_project(label: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "jengine-search-test-{}-{}-{}",
            label,
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn search(root: &Path, q: &str, case: bool, word: bool, rx: bool) -> SearchResults {
        run_search(root, &build_matcher(q, case, word, rx).unwrap())
    }

    #[test]
    fn finds_a_literal_match_with_its_line_number() {
        let d = tmp_project("literal");
        write(&d, "assets/scripts/a.lua", "local x = 1\nprint(\"hello\")\n");
        let r = search(&d, "hello", false, false, false);
        assert_eq!(r.hits.len(), 1, "{:?}", r.hits.len());
        assert_eq!(r.hits[0].path, "assets/scripts/a.lua");
        assert_eq!(r.hits[0].line, 2, "line numbers are 1-based");
        assert_eq!(r.hits[0].matches, vec![(7, 12)]);
        assert_eq!(r.files_matched, 1);
    }

    #[test]
    fn case_insensitive_by_default_and_sensitive_on_request() {
        let d = tmp_project("case");
        write(&d, "a.txt", "Hello\nhello\n");
        assert_eq!(search(&d, "hello", false, false, false).hits.len(), 2);
        assert_eq!(search(&d, "hello", true, false, false).hits.len(), 1);
    }

    #[test]
    fn whole_word_does_not_match_inside_an_identifier() {
        let d = tmp_project("word");
        write(&d, "a.lua", "set_texture()\ntexture = 1\n");
        // Without the flag both lines match; with it, only the standalone use.
        assert_eq!(search(&d, "texture", false, false, false).hits.len(), 2);
        let r = search(&d, "texture", false, true, false);
        assert_eq!(r.hits.len(), 1, "{:?}", r.hits);
        assert_eq!(r.hits[0].line, 2);
    }

    #[test]
    fn whole_word_groups_an_alternation() {
        // `\bfoo|bar\b` would bind the boundaries to one branch each. The
        // non-capturing group is what makes both branches whole-word.
        let d = tmp_project("alt");
        write(&d, "a.txt", "foobar\nfoo bar\n");
        let r = search(&d, "foo|bar", false, true, true);
        assert_eq!(r.hits.len(), 1, "{:?}", r.hits);
        assert_eq!(r.hits[0].line, 2);
    }

    #[test]
    fn regex_mode_is_opt_in() {
        let d = tmp_project("regex");
        write(&d, "a.txt", "a1\nab\n");
        // As a literal, `a.` matches nothing; as a regex it matches both.
        assert_eq!(search(&d, "a.", false, false, false).hits.len(), 0);
        assert_eq!(search(&d, "a.", false, false, true).hits.len(), 2);
    }

    #[test]
    fn an_invalid_regex_is_an_error_not_a_panic() {
        assert!(build_matcher("a(", false, false, true).is_err());
        // The same text as a literal is fine.
        assert!(build_matcher("a(", false, false, false).is_ok());
    }

    #[test]
    fn an_empty_query_is_rejected() {
        // An empty pattern matches every position in every file - the one
        // query guaranteed to hit the cap and return nothing useful.
        assert!(build_matcher("", false, false, false).is_err());
    }

    #[test]
    fn binaries_and_build_output_are_skipped() {
        let d = tmp_project("skip");
        write(&d, "assets/keep.lua", "needle\n");
        write(&d, "target/generated.rs", "needle\n");
        write(&d, "node_modules/dep/index.js", "needle\n");
        write(&d, ".git/COMMIT_EDITMSG", "needle\n");
        // Right extension, so it is never opened at all.
        write(&d, "assets/sprite.png", "needle\n");

        let r = search(&d, "needle", false, false, false);
        assert_eq!(r.hits.len(), 1, "{:?}", r.hits);
        assert_eq!(r.hits[0].path, "assets/keep.lua");
    }

    #[test]
    fn a_non_utf8_file_is_skipped_rather_than_failing_the_search() {
        let d = tmp_project("binary");
        std::fs::write(d.join("blob.dat"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        write(&d, "a.txt", "needle\n");
        let r = search(&d, "needle", false, false, false);
        assert_eq!(r.hits.len(), 1);
    }

    #[test]
    fn an_oversized_file_is_skipped() {
        let d = tmp_project("big");
        let big = "x".repeat(MAX_FILE_BYTES as usize + 16);
        write(&d, "big.txt", &format!("needle\n{big}"));
        write(&d, "small.txt", "needle\n");
        let r = search(&d, "needle", false, false, false);
        assert_eq!(r.hits.len(), 1, "{:?}", r.hits);
        assert_eq!(r.hits[0].path, "small.txt");
    }

    #[test]
    fn a_long_line_is_clipped_and_says_so() {
        let d = tmp_project("clip");
        write(&d, "a.txt", &format!("needle{}", "y".repeat(MAX_LINE_CHARS * 2)));
        let r = search(&d, "needle", false, false, false);
        assert!(r.hits[0].clipped);
        assert_eq!(r.hits[0].text.chars().count(), MAX_LINE_CHARS);
    }

    #[test]
    fn match_offsets_are_characters_not_bytes() {
        // Slicing a JS string by a byte offset splits multi-byte characters,
        // so the highlight would drift right on any line with an accent.
        let d = tmp_project("utf8");
        write(&d, "a.txt", "ääää needle\n");
        let r = search(&d, "needle", false, false, false);
        assert_eq!(r.hits[0].matches, vec![(5, 11)]);
    }

    #[test]
    fn hits_are_capped_and_the_cap_is_reported() {
        let d = tmp_project("cap");
        let body = "needle\n".repeat(MAX_HITS + 100);
        write(&d, "a.txt", &body);
        let r = search(&d, "needle", false, false, false);
        assert!(r.truncated);
        assert_eq!(r.hits.len(), MAX_HITS);
    }

    #[test]
    fn several_matches_on_one_line_are_all_highlighted() {
        let d = tmp_project("multi");
        write(&d, "a.txt", "cat cat cat\n");
        let r = search(&d, "cat", false, false, false);
        assert_eq!(r.hits.len(), 1, "one hit per line, not per match");
        assert_eq!(r.hits[0].matches, vec![(0, 3), (4, 7), (8, 11)]);
    }

    #[test]
    fn results_are_ordered_stably() {
        let d = tmp_project("order");
        write(&d, "b.txt", "needle\n");
        write(&d, "a.txt", "needle\n");
        write(&d, "c.txt", "needle\n");
        let paths: Vec<_> = search(&d, "needle", false, false, false)
            .hits
            .iter()
            .map(|h| h.path.clone())
            .collect();
        assert_eq!(paths, vec!["a.txt", "b.txt", "c.txt"]);
    }

    // ---- replace ----------------------------------------------------------

    fn preview_of(root: &Path, q: &str, r: &str, rx: bool) -> Vec<(u32, String, String)> {
        let m = build_matcher(q, false, false, rx).unwrap();
        let content = std::fs::read_to_string(root.join("a.txt")).unwrap();
        changed_lines(&content, &m, &expand_replacement(r, rx))
    }

    fn apply(root: &Path, q: &str, r: &str, paths: Vec<String>) -> ReplaceSummary {
        apply_replace_in_files(
            root.to_string_lossy().to_string(),
            q.to_string(),
            r.to_string(),
            false,
            false,
            false,
            paths,
        )
        .unwrap()
    }

    #[test]
    fn a_preview_changes_nothing_on_disk() {
        let d = tmp_project("prev");
        write(&d, "a.txt", "keep\nfoo here\nkeep\n");
        let changed = preview_of(&d, "foo", "bar", false);
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert_eq!(changed[0], (2, "foo here".into(), "bar here".into()));
        assert!(
            std::fs::read_to_string(d.join("a.txt")).unwrap().contains("foo"),
            "preview must not write"
        );
    }

    #[test]
    fn a_dollar_in_a_literal_replacement_stays_literal() {
        // `$1` is a capture reference to `regex`; in literal mode the user
        // means the characters.
        let d = tmp_project("dollar");
        write(&d, "a.txt", "PATH\n");
        assert_eq!(preview_of(&d, "PATH", "$PATH", false)[0].2, "$PATH");
    }

    #[test]
    fn regex_mode_expands_capture_groups() {
        let d = tmp_project("caps");
        write(&d, "a.txt", "get_x()\n");
        assert_eq!(preview_of(&d, "get_(\\w+)", "set_$1", true)[0].2, "set_x()");
    }

    #[test]
    fn caret_anchors_to_every_line_not_just_the_file() {
        // Without `multi_line` only line 1 would change while the search panel
        // had listed both - the worst kind of disagreement to ship.
        let d = tmp_project("caret");
        write(&d, "a.txt", "local a\nlocal b\n");
        assert_eq!(preview_of(&d, "^local", "LOCAL", true).len(), 2);
    }

    #[test]
    fn apply_writes_only_the_named_files() {
        let d = tmp_project("apply");
        write(&d, "a.txt", "foo\n");
        write(&d, "b.txt", "foo\n");
        let s = apply(&d, "foo", "bar", vec!["a.txt".into()]);
        assert_eq!(s.files_changed, 1);
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "bar\n");
        assert_eq!(
            std::fs::read_to_string(d.join("b.txt")).unwrap(),
            "foo\n",
            "a file left out of the preview must not be touched"
        );
    }

    #[test]
    fn apply_refuses_to_escape_the_project() {
        let d = tmp_project("escape");
        write(&d, "a.txt", "foo\n");
        let s = apply(&d, "foo", "bar", vec!["../outside.txt".into()]);
        assert_eq!(s.files_changed, 0);
        assert_eq!(s.failures.len(), 1);
        assert!(s.failures[0].contains("escapes"), "{:?}", s.failures);
    }

    #[test]
    fn apply_counts_occurrences_not_lines() {
        let d = tmp_project("count");
        write(&d, "a.txt", "foo foo\nfoo\n");
        assert_eq!(apply(&d, "foo", "x", vec!["a.txt".into()]).replacements, 3);
    }

    #[test]
    fn a_missing_file_is_reported_not_fatal() {
        let d = tmp_project("missing");
        write(&d, "a.txt", "foo\n");
        let s = apply(&d, "foo", "bar", vec!["a.txt".into(), "gone.txt".into()]);
        assert_eq!(s.files_changed, 1);
        assert_eq!(s.failures.len(), 1);
    }

    #[test]
    fn crlf_line_endings_survive_a_replace() {
        // Replacing across whole content rather than rebuilding it from lines
        // is what preserves these; a line-joining implementation would rewrite
        // every file in the project to LF as a side effect.
        let d = tmp_project("crlf");
        write(&d, "a.txt", "foo\r\nbar\r\n");
        apply(&d, "foo", "baz", vec!["a.txt".into()]);
        assert_eq!(
            std::fs::read_to_string(d.join("a.txt")).unwrap(),
            "baz\r\nbar\r\n"
        );
    }
}
