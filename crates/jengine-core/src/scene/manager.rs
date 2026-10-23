//! Scene manager — handles scene loading, scene stack (push/pop), and deferred transitions.

use super::format::SceneData;

/// Actions that can be queued for the next frame boundary.
#[derive(Debug, Clone)]
pub enum SceneAction {
    /// Replace the current scene with a new one loaded from the given path.
    Load(String),
    /// Push the current scene onto the stack and load a new one (e.g. pause menu overlay).
    Push(String),
    /// Pop the top of the scene stack and restore the previous scene.
    Pop,
    /// Shut the game down. Native closes the window; the editor stops Play;
    /// the browser has nothing to close and logs instead.
    Quit,
    /// Multiply the delta time fed to the whole world tick. 0.0 is a true
    /// pause — behaviour graphs, velocity, lifetimes, animation and script
    /// timers all stop together, because they all derive from that one dt.
    SetTimeScale(f32),
}

/// Slice 12.A — a fullscreen fade-overlay request queued by scripts.
/// `color` is RGB 0..=1; alpha is computed by the runner from
/// `duration_s` and the chosen direction.
#[derive(Debug, Clone, Copy)]
pub enum TransitionRequest {
    /// Overlay starts opaque, fades to clear over `duration_s`.
    FadeIn { duration_s: f32, color: [f32; 3] },
    /// Overlay starts clear, fades to opaque over `duration_s`.
    FadeOut { duration_s: f32, color: [f32; 3] },
    /// P1.3 — Screen flash. Starts at `color.a` opacity and decays
    /// linearly to clear over `duration_s`. RGB is the flash tint.
    Flash { duration_s: f32, color: [f32; 4] },
}

/// Manages the active scene and a stack of saved scenes for push/pop transitions.
///
/// Scene transitions are deferred — call `queue_action()` during a frame, then
/// `apply_pending()` at the frame boundary (after all systems have run).
pub struct SceneManager {
    /// The currently active scene data (for snapshot/restore).
    pub current_scene: Option<SceneData>,
    /// Stack of saved scenes for push/pop transitions.
    pub scene_stack: Vec<SceneData>,
    /// Pending action to apply at the next frame boundary.
    pub pending_action: Option<SceneAction>,
    /// Slice 12.A — pending fade-overlay requests. Drained each frame by
    /// the runner. Multiple requests in one frame are preserved in order
    /// so a script can queue `fade_out` then `fade_in` deterministically
    /// (the latest one effectively wins since the runner only renders one
    /// active transition at a time, but draining them all keeps the
    /// signal observable for debugging).
    pub pending_transitions: Vec<TransitionRequest>,
}

impl Default for SceneManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneManager {
    pub fn new() -> Self {
        Self {
            current_scene: None,
            scene_stack: Vec::new(),
            pending_action: None,
            pending_transitions: Vec::new(),
        }
    }

    /// Queue a scene action to be applied at the frame boundary.
    /// Only one action per frame — last one wins.
    pub fn queue_action(&mut self, action: SceneAction) {
        self.pending_action = Some(action);
    }

    /// Take the pending action (if any) for processing by the game loop.
    pub fn take_pending(&mut self) -> Option<SceneAction> {
        self.pending_action.take()
    }

    /// Slice 12.A — queue a fade-overlay request. Multiple per frame are
    /// preserved; the runner drains them with `take_pending_transitions`.
    pub fn queue_transition(&mut self, t: TransitionRequest) {
        self.pending_transitions.push(t);
    }

    /// Slice 12.A — drain queued fade-overlay requests.
    pub fn take_pending_transitions(&mut self) -> Vec<TransitionRequest> {
        std::mem::take(&mut self.pending_transitions)
    }

    /// Push the current scene onto the stack for later restoration.
    pub fn push_current(&mut self, scene_data: SceneData) {
        self.scene_stack.push(scene_data);
    }

    /// Pop the most recently pushed scene from the stack.
    pub fn pop_stack(&mut self) -> Option<SceneData> {
        self.scene_stack.pop()
    }

    /// Set the current scene data (after loading).
    pub fn set_current(&mut self, scene_data: SceneData) {
        self.current_scene = Some(scene_data);
    }

    /// Get the current scene name.
    pub fn current_scene_name(&self) -> &str {
        self.current_scene
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or("")
    }
}
