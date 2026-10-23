//! Runtime debug overlay (feature 3.3).
//!
//! Renders an in-runtime HUD on top of the scene, driven by:
//!
//! - **`F3`** — cycles the stats panel (FPS, frame timings, draw calls,
//!   entity / sprite counts) on and off.
//! - **`F12`** — cycles a compact entity inspector that lists the first
//!   handful of `(Name, Transform, Sprite)` entries in the world.
//! - **`set_debug_overlay(visible)`** — the existing script bridge call
//!   maps `true` → [`DebugOverlayMode::Stats`] and `false` →
//!   [`DebugOverlayMode::Off`] so games can toggle the panel from
//!   gameplay code (e.g. behind a cheat code).
//!
//! The overlay is intentionally minimal: it ships with both
//! `jengine-runtime` (native) and `jengine-web` (browser) so a deployed
//! game can always be inspected without the editor. The editor viewport
//! has its own Svelte panels and is not driven by this module.
//!
//! ## Rendering
//!
//! [`render_debug_overlay_wgpu`] reuses the screen-space sprite batch
//! and the `ui_flush` closure already plumbed by `render_ui_wgpu` /
//! `render_gui_wgpu` — so callers don't need a new render pass, just
//! invoke it after the GUI pass with the same closure. Output is a
//! semi-transparent dark panel (via `UiFlush::Quad(None)`) with text
//! glyphs on top (via `UiFlush::Font`).

use crate::ecs::components::{Name, SpriteComponent, Transform};
use crate::ecs::wgpu_systems::{queue_text_glyphs, UiFlush};
use crate::profiling::FrameStats;
use crate::renderer::backend::wgpu::WgpuSpriteBatch;
use crate::renderer::font_asset::FrameFonts;
use glam::{Vec2, Vec4};
use hecs::World;

/// Debug overlay display mode.
///
/// `Stats` and `Inspector` are mutually exclusive — toggling one always
/// closes the other so a single F-key tap reliably returns the screen
/// to a known state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DebugOverlayMode {
    /// No overlay drawn. Render path is short-circuited.
    #[default]
    Off,
    /// FPS / frame timings / counters panel anchored top-right.
    Stats,
    /// Entity inspector listing anchored top-left.
    Inspector,
}

/// Persistent state for the runtime debug overlay.
///
/// Owned by each runtime (`jengine-runtime` and `jengine-web`) so the
/// mode survives across frames. The state is intentionally tiny — there
/// is no history buffer or scroll position; the overlay re-derives
/// everything from the live `FrameStats` and `World` each frame.
#[derive(Debug, Default, Clone, Copy)]
pub struct DebugOverlayState {
    pub mode: DebugOverlayMode,
    /// F4: outline every collider and joint, and each nav region and walking
    /// agent's path (`debug_draw::physics_debug_commands`).
    /// Independent of `mode`, so it can sit under the F3 stats panel.
    pub physics: bool,
}

impl DebugOverlayState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle the `Stats` panel. If `Inspector` is currently active it
    /// is closed first, so consecutive F3 presses always cycle through
    /// `Off ↔ Stats` without entering `Inspector`.
    /// Toggle the F4 physics outline overlay.
    pub fn toggle_physics(&mut self) {
        self.physics = !self.physics;
    }

    pub fn toggle_stats(&mut self) {
        self.mode = match self.mode {
            DebugOverlayMode::Stats => DebugOverlayMode::Off,
            _ => DebugOverlayMode::Stats,
        };
    }

    /// Toggle the `Inspector` panel. Symmetrical to [`Self::toggle_stats`].
    pub fn toggle_inspector(&mut self) {
        self.mode = match self.mode {
            DebugOverlayMode::Inspector => DebugOverlayMode::Off,
            _ => DebugOverlayMode::Inspector,
        };
    }

    /// Sync the overlay to the script-driven `set_debug_overlay` flag.
    /// `true` activates the stats panel (only when the overlay is
    /// currently `Off`, so a user who pressed F12 doesn't get yanked
    /// out of the inspector); `false` closes whichever panel is open.
    pub fn sync_from_script(&mut self, script_visible: bool) {
        match (script_visible, self.mode) {
            (true, DebugOverlayMode::Off) => self.mode = DebugOverlayMode::Stats,
            (false, DebugOverlayMode::Stats) | (false, DebugOverlayMode::Inspector) => {
                self.mode = DebugOverlayMode::Off;
            }
            _ => {}
        }
    }

    /// True when the overlay should render something this frame.
    pub fn is_visible(&self) -> bool {
        !matches!(self.mode, DebugOverlayMode::Off)
    }
}

/// Maximum entities listed by the inspector — keeps the panel from
/// covering the entire viewport in scenes with thousands of entities.
const INSPECTOR_MAX_ENTRIES: usize = 20;

/// Pixel size of the built-in font glyphs the overlay renders at.
const OVERLAY_FONT_PX: f32 = 14.0;

/// Pixel padding inside the overlay panels (left/right margin, line gap).
const OVERLAY_PADDING: f32 = 8.0;

/// Render the debug overlay through the existing screen-space sprite
/// batch + `ui_flush` closure. The closure must already be bound to a
/// screen-space orthographic projection
/// (`Mat4::orthographic_rh_gl(0, screen_w, 0, screen_h, -1, 1)`),
/// matching the convention used by `render_ui_wgpu` / `render_gui_wgpu`.
///
/// `world` is only inspected when the mode is `Inspector`; `frame_stats`
/// is only read when the mode is `Stats`. Both arguments are accepted
/// unconditionally so the call site stays uniform.
pub fn render_debug_overlay_wgpu(
    state: &DebugOverlayState,
    frame_stats: &FrameStats,
    world: &World,
    fonts: Option<&FrameFonts<'_>>,
    batch: &mut WgpuSpriteBatch,
    screen_w: f32,
    screen_h: f32,
    mut ui_flush: impl FnMut(&mut WgpuSpriteBatch, UiFlush<'_>),
) {
    // The overlay is deliberately always the built-in face: it is engine
    // chrome, not authored content, and it has to stay readable when the
    // project's own font is the thing being debugged.
    let fonts = match (state.mode, fonts) {
        (DebugOverlayMode::Off, _) => return,
        (_, None) => return,
        (_, Some(fonts)) => fonts,
    };
    let fd = fonts.builtin();

    let lines = match state.mode {
        DebugOverlayMode::Stats => build_stats_lines(frame_stats, world),
        DebugOverlayMode::Inspector => build_inspector_lines(world),
        DebugOverlayMode::Off => return,
    };
    if lines.is_empty() {
        return;
    }

    // Measure the widest line so the background panel sizes correctly.
    let glyph_scale = OVERLAY_FONT_PX / fd.pixel_size;
    let line_h = fd.line_height * glyph_scale;
    let mut max_width = 0.0f32;
    for line in &lines {
        let raw: f32 = line
            .chars()
            .filter_map(|ch| {
                fd.glyphs
                    .iter()
                    .find(|g| g.ch == ch)
                    .or_else(|| fd.glyphs.iter().find(|g| g.ch == '?'))
            })
            .map(|g| g.advance)
            .sum();
        let w = raw * glyph_scale;
        if w > max_width {
            max_width = w;
        }
    }
    let panel_w = max_width + OVERLAY_PADDING * 2.0;
    let panel_h = line_h * lines.len() as f32 + OVERLAY_PADDING * 2.0;

    // Screen-space ortho is Y-up with (0,0) bottom-left, so panel center
    // for a top anchor is `screen_h - panel_h * 0.5 - margin`.
    let margin = 10.0;
    let panel_center_y = screen_h - panel_h * 0.5 - margin;
    let panel_center_x = match state.mode {
        DebugOverlayMode::Stats => screen_w - panel_w * 0.5 - margin,
        DebugOverlayMode::Inspector => panel_w * 0.5 + margin,
        DebugOverlayMode::Off => return,
    };
    let panel_center = Vec2::new(panel_center_x, panel_center_y);

    // 1) Dark semi-transparent panel background. `__white` is the
    //    convention `UiFlush::Quad(None)` resolves to in every runtime.
    batch.begin();
    batch.draw_sprite_uv(
        panel_center,
        Vec2::new(panel_w, panel_h),
        0.0,
        Vec4::new(0.0, 0.0, 0.0, 0.65),
        [0.0, 0.0, 1.0, 1.0],
    );
    ui_flush(batch, UiFlush::Quad(None));

    // 2) Text lines, top-down. The font cursor is the top-left of the
    //    line (matches `render_ui_wgpu` text layout), so the first line
    //    sits at `panel_top - padding`.
    let panel_top = panel_center_y + panel_h * 0.5;
    let text_x = panel_center_x - panel_w * 0.5 + OVERLAY_PADDING;
    batch.begin();
    for (i, line) in lines.iter().enumerate() {
        let y = panel_top - OVERLAY_PADDING - line_h * i as f32;
        queue_text_glyphs(
            fd,
            batch,
            line,
            Vec2::new(text_x, y),
            glyph_scale,
            Vec4::new(0.92, 0.95, 1.0, 1.0),
        );
    }
    ui_flush(batch, UiFlush::Font(fonts.builtin_texture_id()));
}

fn build_stats_lines(stats: &FrameStats, world: &World) -> Vec<String> {
    // Prefer the live `world.len()` over `stats.entity_count` so the
    // counter is correct even when no scripting subsystem has been
    // ticked yet (counters are only filled by `update_frame_stats`).
    let live_entities = world.len();
    let mut lines = vec![
        format!("FPS    {:>7.1}", stats.fps),
        format!("frame  {:>6.2}ms", stats.frame_time_ms),
        format!("script {:>6.2}ms", stats.script_time_ms),
        format!("phys   {:>6.2}ms", stats.physics_time_ms),
        format!("render {:>6.2}ms", stats.render_time_ms),
        format!("draws  {:>7}", stats.draw_calls),
        format!("sprites{:>7}", stats.sprite_count),
        format!("partcl {:>7}", stats.particle_count),
        format!("entity {:>7}", live_entities),
    ];
    lines.extend(memory_lines(stats));
    lines
}

/// The memory rows: what the process holds, and - where they differ - what
/// is actually live.
///
/// In a browser `memory_bytes` is reserved wasm linear memory, which only
/// ever grows, so watching it alone makes every scene load look like a leak.
/// `heap` is the live figure that comes back down when a scene is dropped;
/// `peak` is its high-water mark. Off wasm the two agree and only `mem` and
/// `peak` are shown. Everything reads "n/a" when the host installed no
/// tracking allocator, rather than a misleading zero.
fn memory_lines(stats: &FrameStats) -> Vec<String> {
    let mb = |bytes: u64| bytes as f64 / 1_048_576.0;
    if stats.memory_bytes == 0 && stats.memory_live_bytes == 0 {
        return vec!["mem      n/a".to_string()];
    }
    let mut out = vec![format!("mem  {:>6.1}MB", mb(stats.memory_bytes))];
    if stats.memory_live_bytes > 0 && stats.memory_live_bytes != stats.memory_bytes {
        out.push(format!("heap {:>6.1}MB", mb(stats.memory_live_bytes)));
    }
    if stats.memory_peak_bytes > 0 {
        out.push(format!("peak {:>6.1}MB", mb(stats.memory_peak_bytes)));
    }
    out
}

fn build_inspector_lines(world: &World) -> Vec<String> {
    let mut lines = Vec::with_capacity(INSPECTOR_MAX_ENTRIES + 2);
    let total = world.len() as usize;
    lines.push(format!("Entities: {total}"));
    lines.push(String::from("--------------------"));

    // hecs has no by-index iteration; collect a stable id-sorted view.
    let mut ids: Vec<(u32, hecs::Entity)> =
        world.iter().map(|e| (e.entity().id(), e.entity())).collect();
    ids.sort_by_key(|(id, _)| *id);

    for (id, entity) in ids.into_iter().take(INSPECTOR_MAX_ENTRIES) {
        let name = world
            .get::<&Name>(entity)
            .ok()
            .map(|n| n.0.clone())
            .unwrap_or_else(|| String::from("<unnamed>"));

        let pos = world
            .get::<&Transform>(entity)
            .ok()
            .map(|t| format!("({:.0},{:.0})", t.position.x, t.position.y))
            .unwrap_or_default();

        let sprite_tag = if world.get::<&SpriteComponent>(entity).is_ok() {
            " [S]"
        } else {
            ""
        };

        // Keep each entry short — left-pad id and truncate long names.
        let name_short: String = name.chars().take(18).collect();
        lines.push(format!("#{id:<4} {name_short:<18}{sprite_tag} {pos}"));
    }

    if total > INSPECTOR_MAX_ENTRIES {
        lines.push(format!("... +{} more", total - INSPECTOR_MAX_ENTRIES));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_stats_cycles() {
        let mut s = DebugOverlayState::new();
        assert_eq!(s.mode, DebugOverlayMode::Off);
        s.toggle_stats();
        assert_eq!(s.mode, DebugOverlayMode::Stats);
        s.toggle_stats();
        assert_eq!(s.mode, DebugOverlayMode::Off);
    }

    #[test]
    fn toggle_inspector_cycles() {
        let mut s = DebugOverlayState::new();
        s.toggle_inspector();
        assert_eq!(s.mode, DebugOverlayMode::Inspector);
        s.toggle_inspector();
        assert_eq!(s.mode, DebugOverlayMode::Off);
    }

    #[test]
    fn toggle_stats_closes_inspector() {
        let mut s = DebugOverlayState::new();
        s.toggle_inspector();
        assert_eq!(s.mode, DebugOverlayMode::Inspector);
        s.toggle_stats();
        assert_eq!(s.mode, DebugOverlayMode::Stats);
    }

    #[test]
    fn toggle_inspector_closes_stats() {
        let mut s = DebugOverlayState::new();
        s.toggle_stats();
        s.toggle_inspector();
        assert_eq!(s.mode, DebugOverlayMode::Inspector);
    }

    #[test]
    fn sync_from_script_activates_stats_when_off() {
        let mut s = DebugOverlayState::new();
        s.sync_from_script(true);
        assert_eq!(s.mode, DebugOverlayMode::Stats);
    }

    #[test]
    fn sync_from_script_does_not_clobber_inspector() {
        let mut s = DebugOverlayState::new();
        s.toggle_inspector();
        s.sync_from_script(true);
        // F12 inspector wins over a script-driven `true`.
        assert_eq!(s.mode, DebugOverlayMode::Inspector);
    }

    #[test]
    fn sync_from_script_false_closes_any_panel() {
        let mut s = DebugOverlayState::new();
        s.toggle_inspector();
        s.sync_from_script(false);
        assert_eq!(s.mode, DebugOverlayMode::Off);

        s.toggle_stats();
        s.sync_from_script(false);
        assert_eq!(s.mode, DebugOverlayMode::Off);
    }

    #[test]
    fn is_visible_matches_mode() {
        let mut s = DebugOverlayState::new();
        assert!(!s.is_visible());
        s.toggle_stats();
        assert!(s.is_visible());
        s.toggle_inspector();
        assert!(s.is_visible());
    }

    #[test]
    fn memory_rows_report_n_a_without_a_tracking_allocator() {
        let stats = FrameStats::default();
        assert_eq!(memory_lines(&stats), vec!["mem      n/a".to_string()]);
    }

    #[test]
    fn memory_rows_hide_heap_when_it_matches_resident() {
        // Off wasm `resident_bytes` IS the live heap, so a separate row
        // would just repeat the number.
        let stats = FrameStats {
            memory_bytes: 5 * 1024 * 1024,
            memory_live_bytes: 5 * 1024 * 1024,
            memory_peak_bytes: 6 * 1024 * 1024,
            ..Default::default()
        };
        let lines = memory_lines(&stats);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("mem"));
        assert!(lines[1].starts_with("peak"));
    }

    #[test]
    fn memory_rows_show_heap_separately_on_wasm() {
        // Reserved linear memory above the live heap - the case that makes
        // a scene reload look like a leak when only `mem` is shown.
        let stats = FrameStats {
            memory_bytes: 12 * 1024 * 1024,
            memory_live_bytes: 5 * 1024 * 1024,
            memory_peak_bytes: 6 * 1024 * 1024,
            ..Default::default()
        };
        let lines = memory_lines(&stats);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[1].starts_with("heap"), "{lines:?}");
    }
}
