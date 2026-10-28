//! Post-process settings — pure scene data persisted with the scene file.
//!
//! The wgpu post-process pipeline (`renderer::backend::wgpu::postprocess`)
//! reads these settings to build its effect chain. No GPU types here.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::material::UniformValue;

/// Serializable post-process settings for a scene.
///
/// ```json
/// "post_process": {
///   "hdr": true, "tonemap": "aces", "exposure": 1.0,
///   "effects": [ { "name": "bloom", "params": { "threshold": { "Float": 1.0 } } } ]
/// }
/// ```
///
/// `hdr` is opt-in because it changes how light adds up: with it, a light
/// at intensity 2 lights a floor to twice its albedo and the tonemap brings
/// that back into range; without it, light stops at 1.0 as it always has,
/// which is what every existing lit scene was tuned against.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostProcessSettings {
    /// Let light and colour exceed 1.0 until the output pass tonemaps them.
    /// Takes effect only where the adapter offers a float scene target
    /// (`WgpuRenderer::scene_format`); elsewhere the scene draws as LDR.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hdr: bool,
    /// How the output pass maps the scene onto the display. `None` means
    /// [`Tonemap::Aces`] for an HDR scene and [`Tonemap::None`] otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tonemap: Option<Tonemap>,
    /// Multiplies the scene before tonemapping; 1.0 when absent. Only
    /// meaningful with `hdr`. An `Option` so a save writes back exactly what
    /// the file said - `"exposure": 1.0` stays, and an absent one stays
    /// absent (`scene_data_keeps_every_key`). Read it with [`Self::exposure`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f32>,
    /// Enabled effects, in order.
    #[serde(default)]
    pub effects: Vec<PostProcessEffectSettings>,
}

/// The output pass's mapping from scene values to display values. See
/// `assets/shaders/wgsl/output.wgsl` for the curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tonemap {
    /// Clamp to 1.0 - what an 8-bit target always did.
    #[default]
    None,
    /// `c / (1 + c)`.
    Reinhard,
    /// The ACES film curve (Hill's fit): what Unity and Godot ship.
    Aces,
    /// AgX: holds hue in bright colours better than ACES.
    Agx,
}

impl Tonemap {
    /// The value the output shader switches on.
    pub fn shader_index(self) -> f32 {
        match self {
            Tonemap::None => 0.0,
            Tonemap::Reinhard => 1.0,
            Tonemap::Aces => 2.0,
            Tonemap::Agx => 3.0,
        }
    }
}

impl Default for PostProcessSettings {
    fn default() -> Self {
        Self { hdr: false, tonemap: None, exposure: None, effects: Vec::new() }
    }
}

impl PostProcessSettings {
    /// The exposure the output pass applies: the authored one, or 1.0.
    pub fn exposure(&self) -> f32 {
        self.exposure.unwrap_or(1.0).max(0.0)
    }

    /// The tonemap the output pass should apply, given whether HDR is in
    /// effect (asked for, *and* the target can hold it). An LDR frame is
    /// never tonemapped: its values are already in range, and a curve would
    /// only dim them.
    pub fn effective_tonemap(&self, hdr_active: bool) -> Tonemap {
        if !hdr_active {
            return Tonemap::None;
        }
        self.tonemap.unwrap_or(Tonemap::Aces)
    }

    /// The effect chain a scene file declares in its top-level `post_process`
    /// block, or `None` when it declares none.
    ///
    /// **A scene with no block means an empty chain, not "leave what was
    /// there".** The effects are session state the world does not own - like
    /// looping music - so a runtime that only applies a chain and never
    /// replaces it carries the last scene's bloom into every scene after it.
    /// Callers say that explicitly with `.unwrap_or_default()`.
    ///
    /// Parsing is lenient by design: a scene whose block is malformed loses
    /// its effects rather than failing to load.
    pub fn from_scene_json(json: &str) -> Option<Self> {
        Self::from_scene_value(&serde_json::from_str(json).ok()?)
    }

    /// [`Self::from_scene_json`] for a scene already parsed.
    pub fn from_scene_value(scene: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(scene.get("post_process")?.clone()).ok()
    }
}

/// Serializable settings for one post-process effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostProcessEffectSettings {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub params: HashMap<String, UniformValue>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_block_becomes_a_chain() {
        let json = r#"{"name":"s","entities":[],
            "post_process":{"effects":[{"name":"bloom","enabled":true}]}}"#;
        let pp = PostProcessSettings::from_scene_json(json).expect("a chain");
        assert_eq!(pp.effects.len(), 1);
        assert_eq!(pp.effects[0].name, "bloom");
    }

    #[test]
    fn a_scene_without_a_block_has_no_chain() {
        // The load-bearing case: `None` here is what every runtime turns into
        // an empty chain, which is what stops one scene's bloom following the
        // player into the next.
        let json = r#"{"name":"s","entities":[]}"#;
        assert!(PostProcessSettings::from_scene_json(json).is_none());
        assert!(
            PostProcessSettings::from_scene_json(json)
                .unwrap_or_default()
                .effects
                .is_empty()
        );
    }

    #[test]
    fn hdr_is_opt_in_and_defaults_to_the_old_frame() {
        let old = PostProcessSettings::from_scene_json(
            r#"{"entities":[],"post_process":{"effects":[]}}"#,
        )
        .unwrap();
        assert!(!old.hdr);
        assert_eq!(old.exposure(), 1.0);
        assert_eq!(old.effective_tonemap(true), Tonemap::Aces, "hdr defaults to aces when asked");
        assert_eq!(old.effective_tonemap(false), Tonemap::None);

        let hdr = PostProcessSettings::from_scene_json(
            r#"{"entities":[],"post_process":{"hdr":true,"tonemap":"agx","exposure":1.5,"effects":[]}}"#,
        )
        .unwrap();
        assert!(hdr.hdr);
        assert_eq!(hdr.exposure(), 1.5);
        assert_eq!(hdr.effective_tonemap(true), Tonemap::Agx);
        // No float target: the scene draws as LDR, untonemapped.
        assert_eq!(hdr.effective_tonemap(false), Tonemap::None);
    }

    #[test]
    fn the_empty_chain_does_not_zero_the_exposure() {
        assert_eq!(PostProcessSettings::default().exposure(), 1.0);
    }

    #[test]
    fn an_old_block_saves_back_unchanged() {
        let json = r#"{"effects":[{"name":"bloom","enabled":true}]}"#;
        let pp: PostProcessSettings = serde_json::from_str(json).unwrap();
        let back = serde_json::to_string(&pp).unwrap();
        assert!(!back.contains("hdr") && !back.contains("tonemap") && !back.contains("exposure"), "{back}");
    }

    #[test]
    fn a_malformed_block_loses_the_effects_rather_than_the_scene() {
        let json = r#"{"name":"s","entities":[],"post_process":"bloom please"}"#;
        assert!(PostProcessSettings::from_scene_json(json).is_none());
        assert!(PostProcessSettings::from_scene_json("not json at all").is_none());
    }
}
