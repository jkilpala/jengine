//! Build report generator — produces a self-contained HTML report with size analysis.

use std::path::Path;
use std::process::Command;

/// Information about a single file in the build output.
struct FileEntry {
    path: String,
    size: u64,
    category: &'static str,
}

/// Collect all files recursively under `dir`, returning entries with relative paths.
fn collect_files(dir: &Path, base: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    if let Ok(iter) = std::fs::read_dir(dir) {
        for entry in iter.flatten() {
            let path = entry.path();
            if path.is_dir() {
                entries.extend(collect_files(&path, base));
            } else if let Ok(meta) = path.metadata() {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let category = categorize(&rel);
                entries.push(FileEntry {
                    path: rel,
                    size: meta.len(),
                    category,
                });
            }
        }
    }
    entries
}

/// Assign a category label based on file extension/path.
fn categorize(rel_path: &str) -> &'static str {
    if rel_path.ends_with(".wasm") {
        "WASM Binary"
    } else if rel_path.ends_with(".js") {
        "JavaScript"
    } else if rel_path.ends_with(".exe") {
        "Executable"
    } else if rel_path.ends_with(".html") {
        "HTML"
    } else if rel_path.ends_with(".png") || rel_path.ends_with(".jpg") || rel_path.ends_with(".jpeg") || rel_path.ends_with(".webp") {
        "Image"
    } else if rel_path.ends_with(".atlas") {
        "Atlas Metadata"
    } else if rel_path.ends_with("atlas_manifest.json") {
        "Atlas Manifest"
    } else if rel_path.ends_with(".ogg")
        || rel_path.ends_with(".wav")
        || rel_path.ends_with(".mp3")
        || rel_path.ends_with(".song")
    {
        "Audio"
    } else if rel_path.ends_with(".lua") || rel_path.ends_with(".ts") {
        "Script"
    } else if rel_path.ends_with(".json") || rel_path.ends_with(".jengine") {
        "Data"
    } else if rel_path.starts_with("assets/") {
        "Asset (other)"
    } else {
        "Other"
    }
}

/// Format byte count as human-readable string.
fn format_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Per-crate size breakdown entry.
struct CrateEntry {
    name: String,
    size: u64,
}

/// Try to run `twiggy top` on a .wasm file and aggregate sizes by crate.
/// Returns (crate_entries, wasm_total_code_size) or None if twiggy is unavailable.
fn analyze_wasm_crates(wasm_path: &Path) -> Option<(Vec<CrateEntry>, u64)> {
    let output = Command::new("twiggy")
        .args(["top", "-n", "100000", "--format", "json"])
        .arg(wasm_path)
        .output()
        .ok()?;

    if !output.status.success() {
        log::warn!(
            "twiggy failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let items = json.as_array()?;

    let mut crate_sizes: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    let mut total_accounted: u64 = 0;

    for item in items {
        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let size = item
            .get("shallow_size")
            .and_then(|s| s.as_u64())
            .unwrap_or(0);

        if size == 0 {
            continue;
        }

        let crate_name = extract_crate_name(name);
        *crate_sizes.entry(crate_name).or_insert(0) += size;
        total_accounted += size;
    }

    let mut entries: Vec<CrateEntry> = crate_sizes
        .into_iter()
        .map(|(name, size)| CrateEntry { name, size })
        .collect();
    entries.sort_by(|a, b| b.size.cmp(&a.size));

    Some((entries, total_accounted))
}

/// Extract the crate name from a twiggy symbol name.
///
/// Symbols look like:
///   `rapier2d::dynamics::solver::...`
///   `<glam::f32::vec2::Vec2 as core::ops::...>::add`
///   `core::ptr::drop_in_place<alloc::vec::Vec<...>>`
///   `data[0]`, `"function names" subsection`, `code[123]`
fn extract_crate_name(name: &str) -> String {
    // Skip non-Rust items — these are WASM sections/data segments
    if name.starts_with("data[")
        || name.starts_with("code[")
        || name.starts_with("elem[")
        || name.starts_with("type[")
        || name.starts_with("table[")
        || name.contains("subsection")
        || name.starts_with("custom section")
        || name == "\"function names\" subsection"
    {
        return "(wasm overhead)".to_string();
    }

    // Handle `<CrateName::Type as Trait>::method` or `<&mut CrateName::...>`
    let s = name.strip_prefix('<').unwrap_or(name);
    // Strip reference qualifiers: `&mut `, `&`
    let s = s.strip_prefix("&mut ").unwrap_or(s);
    let s = s.strip_prefix('&').unwrap_or(s);

    // Handle `core::ptr::drop_in_place<crate::Type>`
    if let Some(inner) = s.strip_prefix("core::ptr::drop_in_place<") {
        let inner = inner.strip_suffix('>').unwrap_or(inner);
        if let Some(pos) = inner.find("::") {
            return normalize_crate(&inner[..pos]);
        }
    }

    // Standard `crate::module::function` — take first segment
    if let Some(pos) = s.find("::") {
        return normalize_crate(&s[..pos]);
    }

    // Single-word items (wasm intrinsics, etc.)
    "(other)".to_string()
}

/// Normalize crate names: replace hyphens with underscores (Rust convention).
fn normalize_crate(name: &str) -> String {
    // Strip any remaining angle brackets from generics
    let clean = name.split('<').next().unwrap_or(name).trim();
    clean.replace('-', "_")
}

/// Build the HTML section for WASM crate breakdown.
fn build_crate_section(wasm_path: &Path) -> String {
    let Some((entries, total)) = analyze_wasm_crates(wasm_path) else {
        return r#"<h2>WASM Crate Breakdown</h2>
<div class="tips">
  <p style="font-size:12px; color:#a6adc8;">
    Install <code>twiggy</code> for per-crate WASM size analysis:<br>
    <code style="display:inline-block; margin-top:4px; padding:4px 8px; background:#1e1e2e; border-radius:3px;">cargo install twiggy</code>
  </p>
</div>"#
            .to_string();
    };

    if entries.is_empty() {
        return String::new();
    }

    let mut rows = String::new();
    for (i, e) in entries.iter().enumerate() {
        let pct = if total > 0 {
            (e.size as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        // Color-code: top entries get accent color
        let color = if i < 3 { "#f38ba8" } else { "#89b4fa" };
        rows.push_str(&format!(
            "<tr><td class=\"r\">{rank}</td><td class=\"mono\">{name}</td><td class=\"r\">{size}</td><td class=\"r\">{pct:.1}%</td><td><div class=\"bar\" style=\"width:{pct:.1}%;background:{color}\"></div></td></tr>\n",
            rank = i + 1,
            name = e.name,
            size = format_size(e.size),
        ));
    }

    format!(
        r#"<h2>WASM Crate Breakdown</h2>
<p class="note">Sizes derived from WASM symbol names via <code>twiggy</code>. Total analyzed: {total}.</p>
<table>
  <thead><tr><th class="r">#</th><th>Crate</th><th class="r">Size</th><th class="r">%</th><th style="width:30%">Distribution</th></tr></thead>
  <tbody>
{rows}  </tbody>
</table>"#,
        total = format_size(total),
        rows = rows,
    )
}

/// Generate a build report HTML file in the output directory.
///
/// `target` should be `"windows"` or `"web"`.
/// `profile` should be `"release"` or `"debug"`.
/// `engine_path` is used to locate the intermediate (pre-wasm-opt) WASM binary
/// which retains function names needed for per-crate size analysis.
pub fn generate_report(
    output_path: &Path,
    target: &str,
    profile: &str,
    engine_path: Option<&Path>,
    features: Option<&crate::engine_features::Selection>,
) -> anyhow::Result<String> {
    let entries = collect_files(output_path, output_path);

    // Aggregate by category
    let mut categories: std::collections::BTreeMap<&str, (u64, usize)> =
        std::collections::BTreeMap::new();
    for e in &entries {
        let cat = categories.entry(e.category).or_insert((0, 0));
        cat.0 += e.size;
        cat.1 += 1;
    }

    let total_size: u64 = entries.iter().map(|e| e.size).sum();

    // Sort files largest first
    let mut sorted = entries;
    sorted.sort_by(|a, b| b.size.cmp(&a.size));

    // Build category summary rows
    let mut cat_rows = String::new();
    let mut cat_sorted: Vec<_> = categories.iter().collect();
    cat_sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (cat, (size, count)) in &cat_sorted {
        let pct = if total_size > 0 {
            (*size as f64 / total_size as f64) * 100.0
        } else {
            0.0
        };
        cat_rows.push_str(&format!(
            "<tr><td>{cat}</td><td class=\"r\">{count}</td><td class=\"r\">{}</td><td class=\"r\">{pct:.1}%</td><td><div class=\"bar\" style=\"width:{pct:.1}%\"></div></td></tr>\n",
            format_size(*size)
        ));
    }

    // Build file list rows (top 100 largest)
    let mut file_rows = String::new();
    for (i, e) in sorted.iter().take(100).enumerate() {
        let pct = if total_size > 0 {
            (e.size as f64 / total_size as f64) * 100.0
        } else {
            0.0
        };
        file_rows.push_str(&format!(
            "<tr><td class=\"r\">{}</td><td class=\"mono\">{}</td><td>{}</td><td class=\"r\">{}</td><td class=\"r\">{pct:.1}%</td></tr>\n",
            i + 1,
            e.path,
            e.category,
            format_size(e.size)
        ));
    }
    let truncated_note = if sorted.len() > 100 {
        format!(
            "<p class=\"note\">Showing top 100 of {} files.</p>",
            sorted.len()
        )
    } else {
        String::new()
    };

    let timestamp = chrono_free_timestamp();
    let target_label = match target {
        "windows" => "Windows",
        "web" => "Web (WASM)",
        _ => target,
    };

    // For web builds, try to generate WASM crate breakdown
    let crate_section = if target == "web" {
        // Use the intermediate (pre-wasm-opt) WASM that retains symbol names.
        // Falls back to the shipped .wasm if intermediate is not found.
        let profile_dir = if profile == "release" { "release" } else { "debug" };
        let intermediate_wasm = engine_path.map(|ep| {
            crate::cargo_target_dir(ep)
                .join("wasm32-unknown-unknown")
                .join(profile_dir)
                .join("jengine_web.wasm")
        });
        let shipped_wasm = sorted
            .iter()
            .find(|e| e.path.ends_with(".wasm"))
            .map(|e| output_path.join(&e.path));

        let wasm_to_analyze = intermediate_wasm
            .filter(|p| p.exists())
            .or(shipped_wasm);

        wasm_to_analyze
            .map(|p| build_crate_section(&p))
            .unwrap_or_default()
    } else {
        String::new()
    };

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>jEngine Build Report</title>
<style>
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
         background:#1e1e2e; color:#cdd6f4; padding:24px; }}
  h1 {{ font-size:20px; margin-bottom:4px; color:#89b4fa; }}
  h2 {{ font-size:15px; margin:24px 0 8px; color:#a6adc8; border-bottom:1px solid #313244; padding-bottom:4px; }}
  .meta {{ color:#6c7086; font-size:12px; margin-bottom:16px; }}
  .meta span {{ margin-right:16px; }}
  .summary {{ display:flex; gap:16px; margin-bottom:16px; flex-wrap:wrap; }}
  .card {{ background:#313244; border-radius:6px; padding:12px 16px; min-width:120px; }}
  .card .label {{ font-size:11px; color:#6c7086; text-transform:uppercase; letter-spacing:0.5px; }}
  .card .value {{ font-size:22px; font-weight:700; color:#cdd6f4; margin-top:2px; }}
  table {{ width:100%; border-collapse:collapse; font-size:12px; }}
  th {{ text-align:left; padding:6px 8px; border-bottom:2px solid #45475a; color:#a6adc8; font-weight:600; }}
  td {{ padding:5px 8px; border-bottom:1px solid #313244; }}
  tr:hover {{ background:#313244; }}
  .r {{ text-align:right; }}
  .mono {{ font-family:'Cascadia Code','Fira Code',monospace; font-size:11px; word-break:break-all; }}
  .bar {{ height:12px; background:#89b4fa; border-radius:2px; min-width:2px; }}
  .note {{ color:#6c7086; font-size:11px; margin-top:6px; font-style:italic; }}
  .tips {{ background:#313244; border-radius:6px; padding:12px 16px; margin-top:16px; }}
  .tips h3 {{ font-size:13px; color:#f9e2af; margin-bottom:6px; }}
  .tips ul {{ margin-left:16px; font-size:12px; color:#a6adc8; }}
  .tips li {{ margin-bottom:4px; }}
</style>
</head>
<body>
<h1>jEngine Build Report</h1>
<div class="meta">
  <span>Target: <strong>{target_label}</strong></span>
  <span>Profile: <strong>{profile}</strong></span>
  <span>Generated: {timestamp}</span>
</div>

<div class="summary">
  <div class="card"><div class="label">Total Size</div><div class="value">{total}</div></div>
  <div class="card"><div class="label">Files</div><div class="value">{file_count}</div></div>
  <div class="card"><div class="label">Categories</div><div class="value">{cat_count}</div></div>
</div>

<h2>Size by Category</h2>
<table>
  <thead><tr><th>Category</th><th class="r">Files</th><th class="r">Size</th><th class="r">%</th><th style="width:30%">Distribution</th></tr></thead>
  <tbody>
{cat_rows}  </tbody>
</table>

<h2>All Files (by size)</h2>
{truncated_note}
<table>
  <thead><tr><th class="r">#</th><th>File</th><th>Category</th><th class="r">Size</th><th class="r">%</th></tr></thead>
  <tbody>
{file_rows}  </tbody>
</table>

{features_section}

{crate_section}

{tips}
</body>
</html>
"##,
        total = format_size(total_size),
        file_count = sorted.len(),
        cat_count = categories.len(),
        cat_rows = cat_rows,
        file_rows = file_rows,
        truncated_note = truncated_note,
        crate_section = crate_section,
        features_section = features.map(features_section).unwrap_or_default(),
        tips = build_tips(target, profile, &sorted),
    );

    let report_path = output_path.join("build-report.html");
    std::fs::write(&report_path, html)?;
    let report_str = report_path.display().to_string();
    log::info!("Build report written to {}", report_str);
    Ok(report_str)
}

/// The engine features the build carried, why each is in, and what was left
/// out (`engine_features`, docs/engine-features-plan.md).
fn features_section(s: &crate::engine_features::Selection) -> String {
    let esc = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut rows = String::new();
    for name in &s.names {
        let why = s.reasons.get(name).map(|r| r.join("; ")).unwrap_or_default();
        rows.push_str(&format!("    <tr><td>{name}</td><td>in</td><td class=\"mono\">{}</td></tr>\n", esc(&why)));
    }
    for f in s.left_out() {
        rows.push_str(&format!("    <tr><td>{}</td><td>left out</td><td class=\"mono\">nothing in the project uses it</td></tr>\n", f.name));
    }
    format!(
        "<h2>Engine Features</h2>\n<table>\n  <thead><tr><th>Feature</th><th>Built</th><th>Why</th></tr></thead>\n  <tbody>\n{rows}  </tbody>\n</table>\n\
         <p class=\"note\">Chosen from the project's scenes, prefabs, scripts and data; settings.engine_features adjusts it.</p>"
    )
}

/// Generate contextual tips based on the build.
fn build_tips(target: &str, profile: &str, files: &[FileEntry]) -> String {
    let mut tips = Vec::new();

    if target == "web" {
        let wasm_size: u64 = files.iter().filter(|f| f.category == "WASM Binary").map(|f| f.size).sum();
        if profile == "debug" {
            tips.push("Switch to <strong>Release</strong> profile for significantly smaller WASM output (typically 3-5x smaller).");
        }
        // Measured on the empty project (docs/build-sizes.md): what shrinks
        // the download, and what each costs.
        if wasm_size > 1_500_000 {
            tips.push("Serve the <code>.wasm</code> compressed: brotli takes it to about a fifth of its size, gzip to about a third.");
            tips.push("A game without physics can set <code>settings.physics.backend</code> to <code>\"none\"</code>: rapier2d leaves the module (about 0.4 MB gzipped).");
            tips.push("<em>Opt for Size</em> (<code>opt-level = 'z'</code>) makes the module about a third smaller, and the game's code about 3.7x slower - only for a game with CPU to spare.");
        }
        let img_size: u64 = files.iter().filter(|f| f.category == "Image").map(|f| f.size).sum();
        if img_size > 1_000_000 {
            tips.push("Image assets total over 1 MB. Consider compressing textures or using smaller sprite sheets.");
        }
    }

    if target == "windows" && profile == "debug" {
        tips.push("Debug builds include debug symbols and are much larger. Use <strong>Release</strong> for distribution.");
    }

    let audio_size: u64 = files.iter().filter(|f| f.category == "Audio").map(|f| f.size).sum();
    if audio_size > 5_000_000 {
        tips.push("Audio files total over 5 MB. Consider using OGG Vorbis for better compression.");
    }

    if tips.is_empty() {
        return String::new();
    }

    let mut html = String::from("<div class=\"tips\"><h3>💡 Optimization Tips</h3><ul>\n");
    for tip in &tips {
        html.push_str(&format!("  <li>{tip}</li>\n"));
    }
    html.push_str("</ul></div>");
    html
}

/// Simple timestamp without pulling in chrono crate.
fn chrono_free_timestamp() -> String {
    // Use std::time to get a basic timestamp
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs();
            // Basic UTC breakdown
            let days = secs / 86400;
            let time_of_day = secs % 86400;
            let hours = time_of_day / 3600;
            let minutes = (time_of_day % 3600) / 60;
            let seconds = time_of_day % 60;

            // Days since epoch to Y-M-D (simplified leap-year calc)
            let mut y = 1970i64;
            let mut remaining = days as i64;
            loop {
                let days_in_year = if is_leap(y) { 366 } else { 365 };
                if remaining < days_in_year {
                    break;
                }
                remaining -= days_in_year;
                y += 1;
            }
            let month_days: &[i64] = if is_leap(y) {
                &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
            } else {
                &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
            };
            let mut m = 0usize;
            for &md in month_days {
                if remaining < md {
                    break;
                }
                remaining -= md;
                m += 1;
            }
            format!(
                "{y}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                m + 1,
                remaining + 1,
                hours,
                minutes,
                seconds
            )
        }
        Err(_) => "unknown".to_string(),
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
