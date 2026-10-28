//! # jEngine Core
//!
//! Core library for the jEngine 2D game engine.
//! Provides rendering, ECS, scene management, scripting, input, and asset systems.

// Engine code intentionally uses wide function signatures (renderer state,
// system arguments) and complex generic types (ECS queries, glow handles).
// Refactoring these into intermediate structs would obscure call sites
// without improving safety. Suppress the noisy stylistic lints crate-wide.
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

pub mod animation;
pub mod asset;
pub mod behavior;
pub mod data;
/// Full-screen fade / flash overlay, composited after post-processing.
///
/// Lived in `jengine-runtime` until Sep 2026, which is why `fade_in`,
/// `fade_out` and `screen_flash` were preview stubs that only logged in the
/// editor viewport and did nothing at all in web builds.
#[cfg(feature = "gpu-wgpu")]
pub mod fade_overlay;
pub mod debug_draw;
#[cfg(feature = "gpu-wgpu")]
pub mod debug_overlay;
/// Maths that give the same bits on every platform, for simulation.
pub mod detmath;
pub mod ecs;
/// The optional subsystems a game builds only when it uses them, and the
/// `feature!` macro that gates their call sites.
pub mod features;
pub mod input;
pub mod package;
pub mod particles;
pub mod physics;
pub mod audio;
pub mod profiling;
pub mod renderer;
pub mod save;
pub mod scene;
pub mod scripting;
#[cfg(feature = "liveops")]
pub mod liveops;
pub mod locale;
#[cfg(feature = "navigation")]
pub mod navigation;
pub mod tilemap;
pub mod time;
pub mod ui;
pub mod version;
#[cfg(feature = "video")]
pub mod video;
/// Perlin, simplex, cellular and fractal noise (`Noise.*`).
#[cfg(feature = "noise")]
pub mod noise;
/// Window mode, size, vsync, frame cap and the cursor (`Display.*`, `Cursor.*`).
pub mod display;
/// The `log` crate's output in a browser console.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web_log;

/// Engine-wide result type.
pub type EngineResult<T> = anyhow::Result<T>;
