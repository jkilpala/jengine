//! Build & Run: the project's native runtime, compiled and started straight
//! from the project folder - one copy, or several side by side for testing a
//! multiplayer game, as Defold's "Launched Instance Count" does.
//!
//! Each instance gets:
//! - its own save directory, `<project>/.jengine/instances/<n>/saves`
//!   (`JENGINE_SAVE_DIR`), so two copies are two players - a game that keeps
//!   a sign-in token in its saves signs each in as somebody else, and the
//!   same somebody next time;
//! - its cell of the primary monitor (`JENGINE_TILE="<n>/<count>"`, read by
//!   the runtime's `instance_tile`), and its number in its title;
//! - its log, written to `<project>/.jengine/instances/<n>/log.txt`.
//!
//! Nothing is staged: the runtime reads `project.jengine` and `assets/` from
//! its working directory, the project folder, so an edited script or scene
//! is in the next run without a copy.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result};

/// The most instances one run starts.
pub const MAX_INSTANCES: u32 = 4;

/// Where instance `n` (1-based) keeps its saves and its log.
pub fn instance_dir(project: &Path, n: u32) -> PathBuf {
    project.join(".jengine").join("instances").join(n.to_string())
}

/// One launched copy of the game.
pub struct Instance {
    pub n: u32,
    pub child: Child,
    pub log: PathBuf,
}

impl Instance {
    /// Whether it is still running.
    pub fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// The command that starts instance `n` of `count`: the runtime `exe` in
/// the project folder, starting at `scene` (a path under `assets/`) or the
/// project's own start scene.
pub fn instance_command(exe: &Path, project: &Path, scene: Option<&str>, n: u32, count: u32) -> Command {
    let mut cmd = Command::new(exe);
    cmd.current_dir(project)
        .env("JENGINE_SAVE_DIR", instance_dir(project, n).join("saves"))
        .stdin(Stdio::null());
    if count > 1 {
        cmd.env("JENGINE_TILE", format!("{n}/{count}"));
    } else {
        cmd.env_remove("JENGINE_TILE");
    }
    if let Some(scene) = scene.filter(|s| !s.is_empty()) {
        cmd.arg("--scene").arg(Path::new("assets").join(scene));
    }
    cmd
}

/// Start `count` instances of the built runtime (1 to [`MAX_INSTANCES`]).
pub fn launch(exe: &Path, project: &Path, scene: Option<&str>, count: u32) -> Result<Vec<Instance>> {
    let count = count.clamp(1, MAX_INSTANCES);
    let mut started = Vec::new();
    for n in 1..=count {
        let dir = instance_dir(project, n);
        std::fs::create_dir_all(dir.join("saves")).with_context(|| format!("making {}", dir.display()))?;
        let log = dir.join("log.txt");
        let out = File::create(&log).with_context(|| format!("creating {}", log.display()))?;
        let err = out.try_clone()?;
        let child = instance_command(exe, project, scene, n, count)
            .stdout(out)
            .stderr(err)
            .spawn()
            .with_context(|| format!("starting {}", exe.display()))?;
        started.push(Instance { n, child, log });
    }
    Ok(started)
}

/// Compile, then launch. The compile's output goes through
/// [`crate::progress`], as every build's does.
pub fn build_and_run(engine: &Path, project: &Path, scene: Option<&str>, count: u32) -> Result<Vec<Instance>> {
    let exe = crate::windows::compile_native_runtime(engine, project, true)?;
    let count = count.clamp(1, MAX_INSTANCES);
    crate::progress::step(&format!(
        "Starting {count} instance{} of {}",
        if count == 1 { "" } else { "s" },
        scene.unwrap_or("the start scene")
    ));
    launch(&exe, project, scene, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(cmd: &Command, key: &str) -> Option<Option<String>> {
        cmd.get_envs()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn each_instance_has_its_own_saves_and_its_place() {
        let project = Path::new("C:/games/my-game");
        let exe = Path::new("C:/games/my-game/.jengine/runtime/target/release/my-game-runtime.exe");
        let second = instance_command(exe, project, None, 2, 4);
        assert_eq!(second.get_current_dir(), Some(project));
        assert_eq!(env_of(&second, "JENGINE_TILE"), Some(Some("2/4".into())));
        let saves = env_of(&second, "JENGINE_SAVE_DIR").unwrap().unwrap();
        assert!(saves.replace('\\', "/").ends_with(".jengine/instances/2/saves"), "{saves}");
        assert_eq!(second.get_args().count(), 0, "the project's own start scene");
        // One instance is a plain window - and never inherits a tile from
        // the environment the editor itself was started in.
        let only = instance_command(exe, project, Some("scenes/game.scene"), 1, 1);
        assert_eq!(env_of(&only, "JENGINE_TILE"), Some(None));
        let args: Vec<_> = only.get_args().map(|a| a.to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(args, ["--scene", "assets/scenes/game.scene"]);
    }

    /// Real processes: two copies of a tiny program, each in its own folder
    /// and told which it is.
    #[test]
    fn launch_starts_every_instance_with_its_log() {
        let project = std::env::temp_dir().join(format!("jengine-native-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&project);
        std::fs::create_dir_all(&project).unwrap();
        // `cmd /C echo` on Windows, `sh -c` elsewhere: something that prints
        // its environment and exits.
        #[cfg(windows)]
        let exe = PathBuf::from(std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()));
        #[cfg(not(windows))]
        let exe = PathBuf::from("/usr/bin/env");
        let mut started = launch(&exe, &project, None, 2).unwrap();
        assert_eq!(started.len(), 2);
        for inst in &mut started {
            let _ = inst.child.wait();
        }
        for n in 1..=2 {
            assert!(instance_dir(&project, n).join("saves").is_dir());
            assert!(instance_dir(&project, n).join("log.txt").is_file());
        }
        let _ = std::fs::remove_dir_all(&project);
    }
}
