//! The engine's version, and what a project's `engine_version` says about it.
//!
//! `project.jengine` carries the version of the engine that last saved it
//! (`"engine_version": "0.1.0"`, written by `jengine project new` and the
//! editor's save). Before 1.0 a minor version may break the formats, so a
//! later engine can tell what it is opening and migrate it, and an earlier one
//! can say it is reading a project made for a newer engine instead of failing
//! somewhere obscure. A project with no stamp predates it (0.1.0 or earlier).

use std::cmp::Ordering;

/// This engine's version, the workspace's `version`.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `major.minor.patch` as numbers; anything after a `-` or `+` is ignored.
/// `None` for a string that is not a version.
pub fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().unwrap_or(Some(0))?;
    let patch = parts.next().unwrap_or(Some(0))?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// How `a` compares with `b`; `None` when either is not a version.
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    Some(parse(a)?.cmp(&parse(b)?))
}

/// The newer of two versions - what a save writes, so opening a project made
/// by a newer engine and saving it does not claim an older one made it.
/// A version that does not parse loses to one that does.
pub fn newest<'a>(a: &'a str, b: &'a str) -> &'a str {
    match (parse(a), parse(b)) {
        (Some(x), Some(y)) if x >= y => a,
        (Some(_), None) => a,
        _ => b,
    }
}

/// A warning for a project stamped with a version newer than this engine, in
/// a way that may matter: a newer major, or before 1.0 a newer minor. `None`
/// when the project is as old as this engine or older, unstamped, or newer
/// only by a patch.
pub fn newer_project_warning(project_version: Option<&str>) -> Option<String> {
    let project = parse(project_version?)?;
    let engine = parse(ENGINE_VERSION)?;
    let breaking = if engine.0 == 0 { (project.0, project.1) > (engine.0, engine.1) } else { project.0 > engine.0 };
    breaking.then(|| {
        format!(
            "this project was saved by jEngine {} and this is jEngine {ENGINE_VERSION}: \
             parts of it may not load - update the engine",
            project_version.unwrap_or_default()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(parse("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse("1.2"), Some((1, 2, 0)));
        assert_eq!(parse("0.2.0-beta.1"), Some((0, 2, 0)));
        assert_eq!(parse("one"), None);
        assert_eq!(parse("1.2.3.4"), None);
        assert_eq!(compare("0.10.0", "0.9.9"), Some(Ordering::Greater));
        assert_eq!(newest("0.2.0", "0.1.5"), "0.2.0");
        assert_eq!(newest("0.1.0", "0.1.0"), "0.1.0");
        assert_eq!(newest("junk", "0.1.0"), "0.1.0");
    }

    #[test]
    fn only_a_breaking_newer_project_is_warned_about() {
        let (major, minor, patch) = parse(ENGINE_VERSION).unwrap();
        assert_eq!(newer_project_warning(None), None);
        assert_eq!(newer_project_warning(Some(ENGINE_VERSION)), None);
        assert_eq!(newer_project_warning(Some(&format!("{major}.{minor}.{}", patch + 1))), None);
        let breaking = if major == 0 { format!("0.{}.0", minor + 1) } else { format!("{}.0.0", major + 1) };
        assert!(newer_project_warning(Some(&breaking)).is_some_and(|w| w.contains(&breaking)));
    }
}
