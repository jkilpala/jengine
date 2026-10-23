//! Tween system — property-based animations with easing.
//!
//! TweenManager lives alongside EngineApi (not as an ECS component).
//! Scripts create tweens via commands; the tween system ticks and applies them.

use glam::Vec4;
use serde::{Deserialize, Serialize};

use super::easing::EasingFunction;

/// Which property a tween targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TweenTarget {
    Position,
    Rotation,
    Scale,
    SpriteColor,
    SpriteAlpha,
}

/// An active tween — interpolating a property over time.
#[derive(Debug, Clone)]
pub struct ActiveTween {
    pub id: u32,
    pub entity_id: u64,
    pub target: TweenTarget,
    pub start_value: Vec4,
    pub end_value: Vec4,
    pub duration: f32,
    pub elapsed: f32,
    pub easing: EasingFunction,
}

impl ActiveTween {
    /// Compute the current interpolated value.
    pub fn current_value(&self) -> Vec4 {
        let t = if self.duration > 0.0 {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let eased = self.easing.apply(t);
        self.start_value.lerp(self.end_value, eased)
    }

    /// Whether the tween has finished.
    pub fn is_complete(&self) -> bool {
        self.elapsed >= self.duration
    }
}

/// Manages all active tweens. Lives alongside EngineApi in the game state.
#[derive(Debug, Default)]
pub struct TweenManager {
    pub tweens: Vec<ActiveTween>,
    next_id: u32,
}

impl TweenManager {
    pub fn new() -> Self {
        Self {
            tweens: Vec::new(),
            next_id: 1,
        }
    }

    /// Add a new tween. Returns its ID for later cancellation.
    pub fn add_tween(
        &mut self,
        entity_id: u64,
        target: TweenTarget,
        start_value: Vec4,
        end_value: Vec4,
        duration: f32,
        easing: EasingFunction,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.tweens.push(ActiveTween {
            id,
            entity_id,
            target,
            start_value,
            end_value,
            duration,
            elapsed: 0.0,
            easing,
        });
        id
    }

    /// Add a new tween with an externally-supplied ID.
    ///
    /// Used by command-queue-based runtimes (web, viewport) where the script
    /// host pre-allocates tween IDs client-side and posts them with the tween
    /// command — the manager must use the same ID so subsequent
    /// `cancel_tween(id)` calls match.
    ///
    /// `next_id` is bumped past the supplied ID so any later
    /// internally-allocated IDs don't collide.
    pub fn add_tween_with_id(
        &mut self,
        id: u32,
        entity_id: u64,
        target: TweenTarget,
        start_value: Vec4,
        end_value: Vec4,
        duration: f32,
        easing: EasingFunction,
    ) {
        if id >= self.next_id {
            self.next_id = id.saturating_add(1);
        }
        self.tweens.push(ActiveTween {
            id,
            entity_id,
            target,
            start_value,
            end_value,
            duration,
            elapsed: 0.0,
            easing,
        });
    }

    /// Cancel a specific tween by ID.
    pub fn cancel_tween(&mut self, tween_id: u32) {
        self.tweens.retain(|t| t.id != tween_id);
    }

    /// Cancel all tweens for a given entity.
    pub fn cancel_all(&mut self, entity_id: u64) {
        self.tweens.retain(|t| t.entity_id != entity_id);
    }

    /// Advance all tweens by `dt`. Returns completed tween results for the caller
    /// to apply to ECS world. Removes completed tweens.
    pub fn tick(&mut self, dt: f32) -> Vec<TweenResult> {
        let mut results = Vec::new();

        for tween in &mut self.tweens {
            tween.elapsed += dt;
            results.push(TweenResult {
                entity_id: tween.entity_id,
                target: tween.target,
                value: tween.current_value(),
            });
        }

        // Remove completed tweens
        self.tweens.retain(|t| !t.is_complete());

        results
    }

}

/// Result of ticking a tween — the value to apply to the world.
#[derive(Debug, Clone)]
pub struct TweenResult {
    pub entity_id: u64,
    pub target: TweenTarget,
    pub value: Vec4,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tween_lifecycle() {
        let mut mgr = TweenManager::new();
        let id = mgr.add_tween(
            1,
            TweenTarget::Position,
            Vec4::new(0.0, 0.0, 0.0, 0.0),
            Vec4::new(100.0, 200.0, 0.0, 0.0),
            1.0,
            EasingFunction::Linear,
        );

        // Halfway
        let results = mgr.tick(0.5);
        assert_eq!(results.len(), 1);
        assert!((results[0].value.x - 50.0).abs() < 1e-3);
        assert!((results[0].value.y - 100.0).abs() < 1e-3);
        assert_eq!(mgr.tweens.len(), 1);

        // Complete
        let results = mgr.tick(0.6);
        assert_eq!(results.len(), 1);
        assert!((results[0].value.x - 100.0).abs() < 1e-3);
        // Should be removed after completion
        assert_eq!(mgr.tweens.len(), 0);

        let _ = id;
    }

    #[test]
    fn cancel_tween() {
        let mut mgr = TweenManager::new();
        let id = mgr.add_tween(
            1,
            TweenTarget::Scale,
            Vec4::ZERO,
            Vec4::ONE,
            1.0,
            EasingFunction::Linear,
        );
        assert_eq!(mgr.tweens.len(), 1);
        mgr.cancel_tween(id);
        assert_eq!(mgr.tweens.len(), 0);
    }

    #[test]
    fn cancel_all_for_entity() {
        let mut mgr = TweenManager::new();
        mgr.add_tween(1, TweenTarget::Position, Vec4::ZERO, Vec4::ONE, 1.0, EasingFunction::Linear);
        mgr.add_tween(1, TweenTarget::Rotation, Vec4::ZERO, Vec4::ONE, 1.0, EasingFunction::Linear);
        mgr.add_tween(2, TweenTarget::Position, Vec4::ZERO, Vec4::ONE, 1.0, EasingFunction::Linear);
        assert_eq!(mgr.tweens.len(), 3);
        mgr.cancel_all(1);
        assert_eq!(mgr.tweens.len(), 1);
        assert_eq!(mgr.tweens[0].entity_id, 2);
    }

    #[test]
    fn add_tween_with_id_uses_supplied_id_and_advances_next_id() {
        let mut mgr = TweenManager::new();
        // Pre-allocate id 42 (script-host pattern).
        mgr.add_tween_with_id(
            42, 1, TweenTarget::Position,
            Vec4::ZERO, Vec4::ONE, 1.0, EasingFunction::Linear,
        );
        assert_eq!(mgr.tweens.len(), 1);
        assert_eq!(mgr.tweens[0].id, 42);
        // Subsequent internally-allocated ids must not collide with 42.
        let id = mgr.add_tween(
            2, TweenTarget::Rotation,
            Vec4::ZERO, Vec4::ONE, 1.0, EasingFunction::Linear,
        );
        assert!(id > 42, "expected next_id bumped past externally-supplied id");
        // Cancellation matches against the supplied id.
        mgr.cancel_tween(42);
        assert_eq!(mgr.tweens.len(), 1);
        assert_eq!(mgr.tweens[0].id, id);
    }


}
