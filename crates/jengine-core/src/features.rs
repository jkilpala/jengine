//! Engine features: the optional subsystems a game builds only when it uses
//! them (`docs/engine-features-plan.md`).
//!
//! Each is a Cargo feature of `jengine-core` (`fluid`, `dialogue`, ...), all
//! of them under `engine-full`, which is core's default and what the editor
//! viewport and a stock runtime build turn on. This module is the one
//! **description** of them: [`ALL`] says, per feature, which component keys,
//! script namespaces and post effects belong to it - what the builder reads
//! to choose a project's features from its content, and what a load names
//! when a scene uses a component its build left out.
//!
//! The code itself is gated where it is called, in place, with [`feature!`]
//! (or a `#[cfg(feature = "...")]` on a field, variant or arm), so a
//! subsystem's step keeps its exact place in the frame. Nothing has to be
//! remembered: compiling core with an engine feature off fails at every call
//! site that was not wrapped.

/// One engine feature, described.
#[derive(Debug, Clone, Copy)]
pub struct Feature {
    /// The Cargo feature's name.
    pub name: &'static str,
    /// Component keys (scene / prefab JSON) that belong to it.
    pub components: &'static [&'static str],
    /// Script namespaces (`Fluid` in `Fluid.set`) that belong to it.
    pub namespaces: &'static [&'static str],
    /// Post effects (a scene's `post_process` effect `name`) that belong to it.
    pub post_effects: &'static [&'static str],
    /// Folders under `assets/` whose data only it reads (`dialogue/`).
    pub data_dirs: &'static [&'static str],
    /// `project.jengine` settings that switch it on (`asset_packs`): a
    /// project setting one up uses the feature.
    pub settings: &'static [&'static str],
    /// Script calls of its under a namespace it does not own
    /// (`PostProcess.set_lut`): the namespaced name, its flat one found from
    /// `scripting::namespaces`.
    pub calls: &'static [&'static str],
    /// File types whose use is its use (`.ttf`): a string naming such a file
    /// in a scene, prefab, `.gui` document or script.
    pub file_types: &'static [&'static str],
    /// The features it is built on (`portal` on `mirror`), as its Cargo
    /// feature lists them: a build carrying it carries these too.
    pub needs: &'static [&'static str],
    /// The runtimes host it too: `jengine-runtime` and `jengine-web` each
    /// declare a feature of this name - core's, plus their own code and
    /// crates for it (an HTTP client, a video decoder, AccessKit) - which a
    /// build turns on there rather than on `jengine-core`.
    pub host: bool,
}

/// Declares [`ALL`] and [`enabled`] from one table, so a feature cannot be
/// described and not answer, or the other way round.
macro_rules! engine_features {
    (@host) => { false };
    (@host $h:literal) => { $h };
    ($($name:literal: [$($c:literal),*], [$($ns:literal),*], [$($pe:literal),*], [$($dd:literal),*]
        $(, settings [$($st:literal),*])? $(, calls [$($cl:literal),*])? $(, files [$($ft:literal),*])?
        $(, needs [$($nd:literal),*])? $(, host = $host:literal)?;)*) => {
        /// Every engine feature. Keep it in step with `engine-full` in core's
        /// `Cargo.toml` (`every_engine_feature_is_described` checks both ways).
        pub const ALL: &[Feature] = &[$(Feature {
            name: $name,
            components: &[$($c),*],
            namespaces: &[$($ns),*],
            post_effects: &[$($pe),*],
            data_dirs: &[$($dd),*],
            settings: &[$($($st),*)?],
            calls: &[$($($cl),*)?],
            file_types: &[$($($ft),*)?],
            needs: &[$($($nd),*)?],
            host: engine_features!(@host $($host)?),
        }),*];

        /// Whether this build has the feature `name`. An unknown name is not one.
        pub fn enabled(name: &str) -> bool {
            match name {
                $($name => cfg!(feature = $name),)*
                _ => false,
            }
        }
    };
}

// name: [components], [script namespaces], [post effects], [data folders]
//     (, settings [project settings that switch it on]) (, calls [Ns.fn])
//     (, files [".ext"]) (, needs [features it is built on]) (, host = true);
engine_features! {
    "fluid": ["fluid"], ["Fluid"], [], [];
    "dialogue": ["dialogue"], ["Dialogue"], [], ["dialogue"];
    "lens_flare": [], [], ["lens_flare"], [];
    "smoke": ["smoke"], ["Smoke"], [], [];
    "pixel_sim": ["pixel_sim"], ["PixelSim"], [], [];
    "water": ["water_surface"], ["Water"], [], [];
    "flow": ["flow_path"], ["Flow"], [], [];
    "cloth": ["cloth", "rope"], ["Cloth"], [], [];
    "jelly": ["jelly"], ["Jelly"], [], [];
    "displacement": ["displacement_field", "displacer", "field_bend"], [], [], [];
    "rewind": ["rewind", "rewindable"], ["Rewind"], [], [];
    "beat_clock": ["beat_clock"], ["BeatClock"], [], [];
    "wfc": ["wfc"], ["Wfc"], [], [];
    "vision": ["vision"], ["Vision"], [], [];
    "fog_of_war": ["fog_of_war", "sight", "fog_hidden"], ["FogOfWar"], [], [];
    "lightning": ["lightning"], ["Lightning"], [], [];
    "day_cycle": ["day_cycle"], ["DayCycle"], [], [];
    "path_morph": ["path_morph"], ["Morph"], [], [];
    "text_path": ["text_path"], ["TextPath"], [], [];
    "mirror": ["mirror"], ["Mirror"], [], [];
    "portal": ["portal"], ["Portal"], [], [], needs ["mirror"];
    "mode7": ["mode7", "mode7_sprite"], ["Mode7"], [], [];
    "stage": ["stage", "stage_group", "stage_quad", "stage_billboard", "stage_caster"], ["Stage"], [], [];
    "sprite_stack": ["sprite_stack"], [], [], [];
    "sprite_tilt": ["sprite_tilt"], ["SpriteTilt"], [], [];
    "afterimage": ["afterimage"], ["Afterimage"], [], [];
    "silhouette": ["silhouette", "silhouette_occluder"], [], [], [];
    "sprite_outline": ["sprite_outline"], ["Outline"], [], [];
    "snow": ["snow_cover"], ["Snow"], [], [];
    "cast_shadow": ["cast_shadow"], ["CastShadow"], [], [];
    "tile_light": ["tile_light", "tile_light_source"], ["TileLight"], [], [];
    "auto_normal": ["auto_normal"], ["AutoNormal"], [], [];
    "smooth_terrain": ["smooth_terrain"], ["Terrain"], [], [], needs ["noise"];
    "speech_bubble": ["speech_bubble"], ["Bubble"], [], [];
    "toast": ["toast_stack"], ["Toast"], [], [];
    "drag_drop": ["ui_draggable", "ui_drop_slot"], [], [], [];
    "radial_menu": ["ui_radial_menu"], ["Radial"], [], [];
    "behavior_tree": ["behavior_tree"], ["BehaviorTree"], [], [];
    "timeline": ["timeline"], ["Timeline"], [], [];
    "slice": ["sliceable"], [], [], [], needs ["fracture"];
    "offscreen_marker": ["offscreen_marker"], ["Marker"], [], [];
    "trajectory": ["trajectory"], ["Trajectory"], [], [], needs ["line2d"];
    "save_state": [], ["State"], [], [];
    "freeform_light": ["freeform_light"], [], [], [];
    "fog": ["fog"], ["Fog"], [], [];
    "pixelate": [], [], ["pixelate"], [];
    "crt": [], [], ["crt"], [];
    "light_shafts": [], [], ["light_shafts"], [];
    "tilt_shift": [], [], ["tilt_shift", "blur"], [];
    "motion_blur": [], [], ["motion_blur"], [];
    "depth_of_field": [], [], ["depth_of_field"], [];
    "depth_haze": [], [], ["depth_haze"], [];
    "gi": ["global_illumination"], [], [], [];
    "ragdoll": ["ragdoll"], ["Ragdoll"], [], [];
    "packs": [], ["Packs"], [], [], settings ["asset_packs"], host = true;
    "liveops": [], ["LiveOps"], [], [], settings ["liveops"], needs ["packs"], host = true;
    "video": ["video_player"], ["Video"], [], [], host = true;
    "accessibility": ["accessible"], ["Accessibility"], [], [], host = true;
    "fracture": ["breakable"], [], [], [];
    "post_volume": ["post_volume"], [], [], [];
    "trails": ["trail"], ["Trail"], [], [];
    "spline_shape": ["spline_shape"], [], [], [];
    "line2d": ["line"], ["Line"], [], [];
    "sprite_library": ["sprite_library", "sprite_resolver"], [], [], [], calls ["Sprite.set_label"];
    "dynamic_font": [], [], [], [], files [".ttf", ".otf"], host = true;
    "noise": [], ["Noise"], [], [];
    "lut": ["color_lut"], [], [], [], calls ["PostProcess.set_lut"];
    "paint": ["paintable"], [], [], [], calls ["Sprite.paint", "Sprite.press"];
    "behavior": ["behavior_graph"], [], [], [];
    "navigation": ["nav_region", "nav_agent", "nav_obstacle", "nav_link"], ["Nav", "NavLink"], [], [];
    "ik": ["ik_two_bone", "ik_look_at", "ik_chain"], [], [], [], calls ["Skeleton.set_ik_target", "Skeleton.set_ik_weight"];
    "skinned_mesh": ["skinned_mesh"], [], [], [];
    "jiggle": ["jiggle"], [], [], [];
}

/// The feature a component key belongs to, if it belongs to one.
pub fn of_component(key: &str) -> Option<&'static Feature> {
    ALL.iter().find(|f| f.components.contains(&key))
}

/// The feature a post effect belongs to, if it belongs to one.
pub fn of_post_effect(name: &str) -> Option<&'static Feature> {
    ALL.iter().find(|f| f.post_effects.contains(&name))
}

/// Say, once per feature and use, that `what` (`"Fluid.set"`, `"the fluid
/// component"`) needs the feature `name`, which this build left out. A game
/// calling into a missing feature does nothing - the script surface is the
/// same in every build - but never silently.
pub fn missing(name: &str, what: &str) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SAID: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let key = format!("{name}/{what}");
    let fresh = SAID.get_or_init(Default::default).lock().map(|mut s| s.insert(key)).unwrap_or(true);
    if fresh {
        log::warn!(
            "{what} does nothing: this build has no `{name}` engine feature (settings.engine_features; \
             docs/engine-features-plan.md)"
        );
    }
}

/// Run `$body` when this build has the engine feature `$name`; otherwise,
/// with a `$what`, say once that it was left out ([`missing`]).
///
/// ```ignore
/// feature!("fluid", { time("fluids", || crate::ecs::fluid::tick_fluids(world, dt)) });
/// feature!("fluid", "Fluid.set", { crate::ecs::fluid::set_fluid_field(world, e, f, v) });
/// ```
#[macro_export]
macro_rules! feature {
    ($name:literal, $body:block) => {{
        #[cfg(feature = $name)]
        $body
    }};
    ($name:literal, $what:literal, $body:block) => {{
        #[cfg(feature = $name)]
        $body
        #[cfg(not(feature = $name))]
        $crate::features::missing($name, $what)
    }};
}

/// A component no entity has. A query that reads a feature's component
/// beside core ones names this in its place when the feature is left out
/// ([`feature_component!`]), so the query keeps its shape and the
/// `Option` is simply always `None`. `N` keeps two of them apart in one
/// query.
pub struct Absent<const N: u64>;

/// A number for each feature name, so each [`Absent`] stand-in is its own
/// type.
pub const fn absent_id(name: &str) -> u64 {
    let bytes = name.as_bytes();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    hash
}

/// A type alias that is the feature's component when the build has the
/// feature, and an [`Absent`] stand-in when it does not:
///
/// ```ignore
/// crate::feature_component!(type JellyC = "jelly" => crate::ecs::jelly::Jelly);
/// world.query::<(&SpriteComponent, Option<&JellyC>)>()
/// ```
#[macro_export]
macro_rules! feature_component {
    ($vis:vis type $alias:ident = $name:literal => $ty:ty) => {
        #[cfg(feature = $name)]
        $vis type $alias = $ty;
        #[cfg(not(feature = $name))]
        $vis type $alias = $crate::features::Absent<{ $crate::features::absent_id($name) }>;
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_feature_is_described() {
        let manifest = include_str!("../Cargo.toml");
        const START: &str = "\nengine-full = [";
        let at = manifest.find(START).expect("engine-full in core's Cargo.toml");
        let full = &manifest[at + START.len()..];
        let full = &full[..full.find(']').expect("engine-full's closing bracket")];
        let listed: Vec<&str> = full
            .split([',', '\n'])
            .map(|s| s.trim().trim_matches('"'))
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .collect();
        let described: Vec<&str> = ALL.iter().map(|f| f.name).collect();
        assert_eq!(listed, described, "engine-full and features::ALL must name the same features, in order");
        for f in ALL {
            // This test runs with engine-full (core's default), so every
            // described feature must answer `enabled`.
            assert!(enabled(f.name), "{} is described but `enabled` does not know it", f.name);
        }
    }

    /// A feature the runtimes host too is a feature of `jengine-runtime` and
    /// `jengine-web` as well, and in their `engine-full` - or a build asking
    /// either for it fails, or a stock build quietly lacks its host half.
    #[test]
    fn hosted_features_are_the_runtimes_too() {
        let hosts = [
            ("jengine-runtime", include_str!("../../jengine-runtime/Cargo.toml")),
            ("jengine-web", include_str!("../../jengine-web/Cargo.toml")),
        ];
        let hosted: Vec<&str> = ALL.iter().filter(|f| f.host).map(|f| f.name).collect();
        assert!(!hosted.is_empty());
        for (name, manifest) in hosts {
            let full = manifest.lines().find(|l| l.starts_with("engine-full = [")).unwrap_or_else(|| panic!("{name} has no engine-full"));
            for f in &hosted {
                assert!(
                    manifest.lines().any(|l| l.starts_with(&format!("{f} = ["))),
                    "{name} declares no `{f}` feature, which the builder asks it for"
                );
                assert!(full.contains(&format!("\"{f}\"")), "{name}'s engine-full leaves out `{f}`");
                let line = manifest.lines().find(|l| l.starts_with(&format!("{f} = ["))).unwrap();
                assert!(
                    line.contains(&format!("jengine-core/{f}")),
                    "{name}'s `{f}` does not turn on core's: {line}"
                );
            }
        }
    }

    /// `needs` is what core's Cargo feature turns on beside its own
    /// crates, so the builder's list and report say what a build carries.
    #[test]
    fn needs_match_the_cargo_features() {
        let manifest = include_str!("../Cargo.toml");
        for f in ALL {
            let line = manifest
                .lines()
                .find(|l| l.starts_with(&format!("{} = [", f.name)))
                .unwrap_or_else(|| panic!("no `{}` feature in core's Cargo.toml", f.name));
            let mut cargo: Vec<&str> = line[line.find('[').unwrap() + 1..line.rfind(']').unwrap()]
                .split(',')
                .map(|s| s.trim().trim_matches('"'))
                .filter(|s| !s.is_empty() && !s.starts_with("dep:"))
                .collect();
            cargo.sort();
            let mut needs = f.needs.to_vec();
            needs.sort();
            assert_eq!(cargo, needs, "`{}`: Cargo turns on {cargo:?}, the table says it needs {needs:?}", f.name);
        }
    }

    #[test]
    fn components_and_effects_find_their_feature() {
        assert_eq!(of_component("fluid").map(|f| f.name), Some("fluid"));
        assert_eq!(of_post_effect("lens_flare").map(|f| f.name), Some("lens_flare"));
        assert!(of_component("sprite").is_none(), "core components belong to no feature");
    }
}
