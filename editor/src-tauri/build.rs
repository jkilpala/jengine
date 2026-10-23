//! Tauri's own build step, plus what the editor's About box says about the
//! build it is: the git commit and the date it was built.
//!
//! Both are baked in as `JENGINE_GIT_COMMIT` / `JENGINE_BUILD_DATE` and read
//! with `env!` in `commands/help.rs`. Without git (a source archive, a CI
//! checkout with no `.git`) the commit is `unknown`. The date honours
//! `SOURCE_DATE_EPOCH`, so a reproducible build can pin it.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Days since 1970-01-01 to a civil (year, month, day) - Howard Hinnant's
/// `civil_from_days`, so the build needs no date crate.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn main() {
    let commit = git(&["rev-parse", "--short=10", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=JENGINE_GIT_COMMIT={commit}");

    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
        });
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    println!("cargo:rustc-env=JENGINE_BUILD_DATE={y:04}-{m:02}-{d:02}");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    // Rebuild the stamp when the checked-out commit moves.
    // `--git-path` answers for a linked worktree too, whose refs live in
    // the main checkout.
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&["rev-parse", "--git-path", &branch])
    {
        println!("cargo:rerun-if-changed={path}");
    }

    tauri_build::build()
}
