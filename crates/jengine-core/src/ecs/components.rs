//! Built-in ECS components for jEngine.

use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::asset::{MaterialId, TextureId};

/// 2D transform: position, rotation, and scale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    pub position: Vec2,
    /// Rotation in radians.
    pub rotation: f32,
    pub scale: Vec2,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }
}

/// Cached world-space transform, derived from a [`Transform`] plus the
/// chain of ancestor `Transform`s reached through [`Parent`] references.
///
/// Recomputed each frame by
/// [`crate::ecs::hierarchy::resolve_world_transforms`]. Sprite rendering
/// uses this when present; behavior systems and physics still mutate the
/// local [`Transform`].
///
/// A world transform is a full 2D affine map: a local point `p` lands at
/// `position + R(rotation) * K(shear) * (scale * p)`, where `K(k)` is
/// `[[1, k], [0, 1]]`. `shear` is zero unless a turned child sits under a
/// non-uniformly scaled (or sheared) parent - a rune circle spinning on a
/// floor squashed into perspective. Before Sep 2026 there was no shear, so
/// such a child turned as a whole ellipse instead of turning *within* its
/// parent's squash (Godot's `Transform2D` and Unity's matrices both shear).
/// See [`crate::ecs::hierarchy::compose`].
///
/// Foundation slice of `docs/feature-backlog.md` item 3.5; the bone /
/// animation / Spine slices build on top.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldTransform {
    pub position: Vec2,
    /// Rotation in radians.
    pub rotation: f32,
    pub scale: Vec2,
    /// Shear between the scale and the rotation (see the type's docs).
    #[serde(default)]
    pub shear: f32,
}

impl WorldTransform {
    /// A local vector through this transform's linear part - scale, shear,
    /// rotation - without the translation.
    #[inline]
    pub fn apply_linear(&self, v: Vec2) -> Vec2 {
        let s = v * self.scale;
        let k = Vec2::new(s.x + self.shear * s.y, s.y);
        // libm's, not the platform's: a child's place feeds the simulation.
        let (sin, cos) = crate::detmath::DetMath::dsin_cos(self.rotation);
        Vec2::new(k.x * cos - k.y * sin, k.x * sin + k.y * cos)
    }

    /// A local point in world space.
    #[inline]
    pub fn apply(&self, p: Vec2) -> Vec2 {
        self.position + self.apply_linear(p)
    }
}

impl Default for WorldTransform {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
            shear: 0.0,
        }
    }
}

impl From<&Transform> for WorldTransform {
    fn from(t: &Transform) -> Self {
        Self {
            position: t.position,
            rotation: t.rotation,
            scale: t.scale,
            shear: 0.0,
        }
    }
}

/// Entity-parenting link. Attaching this to a child entity tells the
/// hierarchy system to compose the child's local [`Transform`] under the
/// parent's [`WorldTransform`] each frame.
///
/// Stored as a `hecs::Entity` for direct lookup at runtime. Scene
/// serialization writes it out as a *name reference* (the parent entity's
/// `name` field) and resolves back to an `Entity` on load — see
/// `crate::scene::format::load_scene`.
///
/// Cycles are detected and broken at resolution time (a cycle leaves the
/// child's `WorldTransform` equal to its local `Transform` for that frame
/// and emits a `log::warn!`).
#[derive(Debug, Clone, Copy)]
pub struct Parent(pub hecs::Entity);

/// Marks an entity as a bone within a skeleton.
///
/// Bones are ordinary parented entities (slice 3.5.A) carrying a
/// [`Transform`]. This component layers the *skeleton-specific* metadata
/// on top:
/// - `name`: stable identifier for lookup via
///   [`crate::ecs::skeleton::find_bone`] and animation clips (slice 3.5.C).
///   Unique per skeleton; scripts and animations resolve bones by name.
/// - `length`: nominal length of the bone in local units, used for IK
///   targeting (slice 3.5.F), gizmo drawing, and mesh skinning weight
///   defaults (slice 3.5.E). `0.0` is allowed for tip bones.
///
/// A bone's parent in the skeleton tree is the standard `Parent`
/// component — there is no separate `parent_bone` field.
///
/// Foundation for the Spine / skeletal animation stack
/// (`docs/feature-backlog.md` item 3.5, slice B).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Bone {
    pub name: String,
    #[serde(default)]
    pub length: f32,
}

/// Marks the root entity of a skeleton.
///
/// The root is itself an ordinary entity with a [`Transform`] (often
/// representing the character's pelvis or rig origin). Its descendant
/// entities carrying [`Bone`] components form the skeleton tree.
///
/// `name` identifies the skeleton instance for tooling and scripts; it is
/// independent of the entity name to allow multiple skeletons of the same
/// kind in a scene.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkeletonRoot {
    pub name: String,
}

// The IK constraints (`ik_two_bone`, `ik_chain`, `ik_look_at`) are the `ik`
// engine feature's, in `animation::ik`; named here too where the build has it.
#[cfg(feature = "ik")]
pub use crate::animation::ik::{IkChain, IkGround, IkLookAt, IkTwoBone};

/// One vertex of a [`Mesh2D`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshVertex {
    /// Local to the entity's `Transform`: the mesh moves, turns and scales
    /// with its entity, and whatever deforms it writes only the shape.
    pub position: [f32; 2],
    /// 0..1 across the texture - or across the `region`, when the mesh
    /// names one - with `v = 0` at the top, as a sprite's `uv_rect` has it.
    pub uv: [f32; 2],
    /// Multiplied with the component's `color`. Per vertex, so a surface
    /// can darken toward its far edge without a second texture.
    pub color: [f32; 4],
    /// Perspective weight, 1 for a flat mesh. For a quad that stands in
    /// depth - the side wall of a corridor - give the far edge's vertices
    /// `far / near` (the ratio of the edge lengths) and the texture is
    /// mapped in perspective rather than bent along the quad's diagonal.
    /// Anything not finite or not positive is drawn as 1.
    pub q: f32,
}

impl Default for MeshVertex {
    fn default() -> Self {
        Self { position: [0.0, 0.0], uv: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0], q: 1.0 }
    }
}

/// A textured polygon: what a sprite is, with the corners free.
///
/// Drawn **in the sprite pass**, sorted with the sprites, shadows and world
/// text by `z_order`, batched with sprites of the same texture, and put
/// through the entity's `MaterialComponent` like a sprite. That is the
/// difference from `VectorPath` (flat colour, its own pass, always over
/// every sprite) and from `SkinnedMesh` (needs a skeleton, its own pass),
/// and it is why a mesh can be a wall in a first-person corridor with a
/// monster sprite standing in front of it. `SpriteComponent::corners` is
/// the four-vertex, one-colour, affine-textured special case of this.
///
/// Nothing in the engine writes the vertices: a package or a script does,
/// per frame if it likes. That is the point - a corridor's walls, a flag in
/// the wind and a jelly are all "a system that writes a `Mesh2D`".
///
/// A mesh is never a backdrop: it is not drawn beneath the tilemaps however
/// low its z, because the tilemap pass draws sprites only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Mesh2D {
    pub texture_id: String,
    /// A named region of that texture's atlas, as `SpriteComponent.region`;
    /// resolved once into `uv_rect` by `resolve_sprite_regions`. Change it
    /// and clear `uv_rect` and `region_resolved` to re-point a pooled mesh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// The part of the texture the vertices' `uv` span, `[u0, v0, u1, v1]`;
    /// the whole texture when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uv_rect: Option<[f32; 4]>,
    #[serde(skip)]
    pub region_resolved: bool,
    pub vertices: Vec<MeshVertex>,
    /// A triangle list into `vertices`. **Empty means a fan** over the
    /// vertices in order, so a convex polygon - a quad - needs no indices.
    /// A pair of triangles `a b c, a c d` costs one sprite slot; any other
    /// triangle costs one of its own.
    pub indices: Vec<u32>,
    /// Multiplied with every vertex's colour.
    pub color: [f32; 4],
    pub z_order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
}

impl Default for Mesh2D {
    fn default() -> Self {
        Self {
            texture_id: Default::default(),
            region: None,
            uv_rect: None,
            region_resolved: false,
            vertices: Vec::new(),
            indices: Vec::new(),
            color: [1.0, 1.0, 1.0, 1.0],
            z_order: 0,
            sort_layer: None,
        }
    }
}

impl Mesh2D {
    /// A quad from four local corners in order `[TL, TR, BR, BL]`, the
    /// texture mapped corner to corner as a sprite maps its `uv_rect`.
    pub fn quad(texture_id: impl Into<String>, corners: [[f32; 2]; 4]) -> Self {
        let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        Self {
            texture_id: texture_id.into(),
            vertices: corners
                .iter()
                .zip(uvs)
                .map(|(p, uv)| MeshVertex { position: *p, uv, ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    /// The triangle list to draw: `indices`, or a fan when there are none.
    pub fn triangles(&self) -> std::borrow::Cow<'_, [u32]> {
        if !self.indices.is_empty() {
            return std::borrow::Cow::Borrowed(&self.indices);
        }
        let n = self.vertices.len() as u32;
        let mut fan = Vec::with_capacity((n.saturating_sub(2) * 3) as usize);
        for i in 1..n.saturating_sub(1) {
            fan.extend_from_slice(&[0, i, i + 1]);
        }
        std::borrow::Cow::Owned(fan)
    }

    /// How far the farthest vertex is from the entity's origin, in local
    /// units - a disc that covers the mesh, for culling.
    pub fn bounding_radius(&self) -> f32 {
        self.vertices
            .iter()
            .map(|v| (v.position[0] * v.position[0] + v.position[1] * v.position[1]).sqrt())
            .fold(0.0, f32::max)
    }
}

/// Sprite rendering component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteComponent {
    /// Asset path or identifier for the texture - interned
    /// (`asset::TextureId`), so sorting and batching sprites by texture
    /// compares indices, not strings. Reads and writes as the string.
    pub texture_id: TextureId,
    /// RGBA tint color (1.0 = full intensity).
    pub color: [f32; 4],
    /// Draw order — higher z_order draws on top, against sprites and other
    /// text alike: world text is drawn by the interleaved sprite pass since
    /// Sep 2026. Before that it was its own pass after every sprite pass, so
    /// a label at z 5 drew over a sprite at z 50. Tilemaps are still their
    /// own pass, before the sprites.
    pub z_order: i32,
    /// Sprite size in world units, before the transform's scale.
    ///
    /// `None` draws at the texture's own pixel size - the part of it the
    /// sprite shows, so a `uv_rect`, an atlas `region` or an animation frame
    /// draws at that cell's size. See [`SpriteComponent::drawn_size`] for the
    /// exceptions. (Until Sep 2026 `None` always meant 64 x 64, whatever the
    /// texture; scenes written against that were migrated to an explicit
    /// `[64, 64]`, so they look the same.)
    pub size: Option<[f32; 2]>,
    /// UV rectangle for sprite atlas regions: [u_min, v_min, u_max, v_max].
    /// Defaults to full texture [0, 0, 1, 1].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_rect: Option<[f32; 4]>,
    /// Name of a region in the atlas registered under `texture_id`. When set,
    /// `uv_rect` is filled in from that region by
    /// [`crate::ecs::systems::resolve_sprite_regions`], so a sprite says which
    /// part of a sheet it draws by name rather than in coordinates that go
    /// silently wrong when the sheet is re-laid-out.
    ///
    /// The atlas has to be loaded (a scene's `atlas_refs`); until then the
    /// sprite draws with whatever `uv_rect` it was given. The animation
    /// equivalent is `AnimationFrame::region`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Whether `region` has been resolved. Runtime state: cleared by
    /// deserialization, so a loaded or spawned sprite resolves once.
    #[serde(skip)]
    pub region_resolved: bool,
    /// Anchor/pivot point as a fraction of sprite size (0.0–1.0).
    /// [0.5, 0.5] = center (default). Position is placed at this point.
    #[serde(default = "default_anchor")]
    pub anchor: [f32; 2],
    /// How the sprite's `size` is filled with the source texture.
    /// `None` (default) and `Some(SpriteFill::Stretch)` both render a
    /// single quad covering `size` with UV `[0,0,1,1]` (modulated by
    /// `uv_rect`). See [`SpriteFill`] for `NineSlice` and `Tile` modes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<SpriteFill>,
    /// Optional sort-layer name. When set, the active `.sortlayers`
    /// asset's `base_z` is added to `z_order` at render time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Sort this sprite against the others in its band by its world
    /// position instead of leaving them in an arbitrary order: lower on
    /// screen draws in front.
    ///
    /// This is what a game needs whenever y is standing in for depth - a
    /// beat-em-up's walkable floor, an isometric or top-down scene where a
    /// character passes behind a tree. Without it the only order available
    /// is `z_order`, which is fixed at authoring time and so cannot say
    /// "whoever is nearer the camera right now".
    ///
    /// It sorts **within** the band `z_order` and `sort_layer` put the
    /// sprite in, never across bands: a backdrop below the actors stays
    /// below them however far down the screen it is drawn. In one band a
    /// sprite that does not opt in sorts as though its y were 0, so a band
    /// mixing the two is only predictable if the plain sprites belong
    /// above or below everything - give them their own `z_order` instead.
    ///
    /// What sorts is the entity's **origin**, not the middle of the quad,
    /// so anchor a character at its feet (`anchor: [0.5, 0.0]`) and it
    /// passes another exactly when they cross.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub y_sort: bool,
    /// Optional explicit quad corners as local-space offsets from the
    /// entity's world position, in order `[TL, TR, BR, BL]`. When `Some`,
    /// the renderer draws this exact (possibly non-rectangular) quad and
    /// ignores `size`, `anchor`, `fill`, and rotation — used for faked
    /// perspective walls / trapezoids. The active `uv_rect` is still mapped
    /// corner-to-corner. Set from scripts via `set_sprite_corners`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corners: Option<[[f32; 2]; 4]>,
    /// Pixel size of the texture named by `texture_px_for`, filled by
    /// [`crate::ecs::systems::resolve_sprite_regions`] once it is loaded.
    /// Runtime state, for [`SpriteComponent::drawn_size`].
    #[serde(skip)]
    pub texture_px: Option<[f32; 2]>,
    /// Which texture `texture_px` was measured from, so a texture swap
    /// (`set_sprite_region`, an animation frame) is measured again.
    #[serde(skip)]
    pub texture_px_for: TextureId,
}

impl SpriteComponent {
    /// What a sprite with no `size` draws at when its texture's size is not
    /// known: the built-in white texture (a plain coloured quad - its real
    /// size is one pixel), or a texture that has not loaded yet.
    pub const FALLBACK_SIZE: [f32; 2] = [64.0, 64.0];

    /// The size this sprite draws at before the transform's scale, when it
    /// can be known: `size` if set, else the texture's pixel size times the
    /// span of its `uv_rect`. `None` when neither is available yet.
    pub fn natural_size(&self) -> Option<glam::Vec2> {
        if let Some([w, h]) = self.size {
            return Some(glam::Vec2::new(w, h));
        }
        let [tw, th] = self.texture_px?;
        let uv = self.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        Some(glam::Vec2::new(tw * (uv[2] - uv[0]).abs(), th * (uv[3] - uv[1]).abs()))
    }

    /// How far down the screen this sprite sorts inside its band, as the
    /// secondary half of a render sort key.
    ///
    /// Negated, so that a *lower* world y - nearer the camera in a game
    /// where y is depth - gives a *larger* key and therefore draws later,
    /// on top. Quantised to 1/16 of a unit so the key is an integer and
    /// the sort stays cheap and total; two sprites within a sixteenth of a
    /// unit keep the order the query gave them, which is stable.
    ///
    /// 0 when the sprite has not opted in, which is what keeps a band of
    /// ordinary sprites batched together by texture rather than split by
    /// position.
    pub fn y_sort_key(&self, world_y: f32) -> i32 {
        if !self.y_sort {
            return 0;
        }
        // Clamped rather than cast raw: `as` on a value past i32's range
        // saturates in Rust, but a NaN y would become 0 and quietly sort
        // into the middle of the band, so fold it to the back instead.
        if world_y.is_nan() {
            return i32::MAX;
        }
        (-world_y * 16.0).clamp(i32::MIN as f32, i32::MAX as f32) as i32
    }

    /// The size this sprite draws at before the transform's scale:
    /// [`natural_size`](Self::natural_size), or `fallback` (normally
    /// [`FALLBACK_SIZE`](Self::FALLBACK_SIZE)) when that is not known.
    pub fn drawn_size(&self, fallback: glam::Vec2) -> glam::Vec2 {
        self.natural_size().unwrap_or(fallback)
    }
}

fn default_anchor() -> [f32; 2] {
    [0.5, 0.5]
}

impl Default for SpriteComponent {
    fn default() -> Self {
        Self {
            texture_id: TextureId::default(),
            region: None,
            region_resolved: false,
            color: [1.0, 1.0, 1.0, 1.0],
            z_order: 0,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: None,
            y_sort: false,
            corners: None,
            texture_px: None,
            texture_px_for: TextureId::default(),
        }
    }
}
/// How a sprite's drawn area is filled with its source texture.
///
/// Border / tile-size values are expressed in the **same units as
/// `SpriteComponent::size`** (world units).
///
/// A nine-slice border means the same number of **texture pixels**: a
/// 16-unit border draws the texture's outer 16 pixels 16 units wide, however
/// large the sprite. So the corners keep their shape and only the edges and
/// centre stretch. The texture's size is looked up once it is loaded
/// (`NineSlice::source_px`, filled by `resolve_sprite_regions`); until then
/// the sprite draws stretched.
///
/// Until Sep 2026 the UV slice was `border / size` - the *drawn* size - which
/// makes every cell sample exactly the part of the texture a plain stretch
/// would. NineSlice drew identically to Stretch (test-lab BUGS #96).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SpriteFill {
    /// Single quad covering the sprite (default behaviour).
    #[default]
    Stretch,
    /// Nine-cell grid: four fixed corners, two horizontal & two vertical
    /// edges that stretch, and one stretched center. Border values are
    /// thicknesses on each side; if the borders exceed the sprite's
    /// size on either axis they are silently clamped.
    NineSlice {
        #[serde(default)]
        left: f32,
        #[serde(default)]
        right: f32,
        #[serde(default)]
        top: f32,
        #[serde(default)]
        bottom: f32,
        /// Pixel size of the sampled texture area (the `uv_rect` region, or
        /// the whole texture). Resolved at runtime, never authored.
        #[serde(skip)]
        source_px: Option<[f32; 2]>,
    },
    /// Repeat the source texture across the sprite's `size` in tiles of
    /// `tile_size` world units. Partial tiles on the right/bottom edges
    /// are UV-clipped to avoid cutoff artifacts.
    Tile { tile_size: [f32; 2] },
    /// The first `amount` (0..1) of the sprite from one side - a health bar,
    /// a charge. Texture and all are cropped (`ecs::sprite_fill`).
    Bar {
        #[serde(default)]
        amount: f32,
        #[serde(default)]
        from: FillFrom,
    },
    /// The first `amount` (0..1) of a turn about the sprite's middle, from
    /// `start` (radians: 0 along +x, `PI / 2` up), clockwise or not - a
    /// cooldown, a warning that fills before a strike (`ecs::sprite_fill`).
    Radial {
        #[serde(default)]
        amount: f32,
        #[serde(default = "radial_start")]
        start: f32,
        #[serde(default = "default_true_fill")]
        clockwise: bool,
    },
}

fn radial_start() -> f32 {
    std::f32::consts::FRAC_PI_2
}

fn default_true_fill() -> bool {
    true
}

/// Which side a [`SpriteFill::Bar`] fills from.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FillFrom {
    #[default]
    Left,
    Right,
    Bottom,
    Top,
}

impl SpriteComponent {
    /// Set a bar or radial fill's `amount` (`Sprite.set_fill`); `false`
    /// when the sprite has no such fill.
    pub fn set_fill_amount(&mut self, value: f32) -> bool {
        match self.fill.as_mut() {
            Some(SpriteFill::Bar { amount, .. }) | Some(SpriteFill::Radial { amount, .. }) => {
                *amount = value.clamp(0.0, 1.0);
                true
            }
            _ => false,
        }
    }
}

/// Which scripting language a script uses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScriptLanguage {
    Lua,
    JavaScript,
}

/// Script component — attaches a script to an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptComponent {
    pub language: ScriptLanguage,
    /// Path to the script source file (relative to project assets).
    pub source_path: String,
    /// Optional cold-entity culling radius in world units. When `Some(r)`,
    /// the script's `update()` is skipped on frames where the entity is
    /// more than `r` units away from the active camera. `init()` and named
    /// callbacks (timers, events, GUI clicks) still fire normally — only
    /// the per-frame `update` is gated. `None` (default) disables culling
    /// and runs every frame, matching pre-culling behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cull_distance: Option<f32>,
}

/// Human-readable name for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Name(pub String);

pub use crate::ui::TextAlign;

fn is_left(a: &TextAlign) -> bool {
    *a == TextAlign::Left
}

/// Text rendering component — renders text using a bitmap font.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextComponent {
    /// The text string to display.
    pub text: String,
    /// Path to a `.font` asset, e.g. `"fonts/ui.font"`. Empty uses the
    /// built-in font.
    ///
    /// The atlas image is the **sibling file with the same stem**
    /// (`fonts/ui.font` -> `fonts/ui.png`); a `.font` names no texture of its
    /// own. Each runtime scans the world for the ids its entities reference
    /// and loads them, so naming one here is the whole of the wiring - there
    /// is no scene-level refs array to keep in step.
    ///
    /// A font that fails to load falls back to the built-in face rather than
    /// dropping the text.
    #[serde(default)]
    pub font_id: String,
    /// Font scale (1.0 = native pixel size).
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
    /// RGBA tint color.
    #[serde(default = "default_text_color")]
    pub color: [f32; 4],
    /// Draw order — higher z_order draws on top.
    #[serde(default)]
    pub z_order: i32,
    /// Optional sort-layer name (see `SortLayers`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Wrap the text at this width in world units. `0.0` (the default)
    /// means no wrapping — only explicit newlines break the line, which is
    /// how text behaved before the layout pass existed.
    #[serde(default)]
    pub wrap_width: f32,
    /// Horizontal alignment of each line about the transform's x - see
    /// [`TextAlign`]. Written as `"align": "center"`; omitted means left,
    /// which is how every scene authored before Sep 2026 draws.
    #[serde(default, skip_serializing_if = "is_left")]
    pub align: TextAlign,
    /// Read `text` as markup - `[color=#e05040]`, `[b]`, `[wave]`,
    /// `[shake]`, `[rainbow]`, `[pause=0.5]`, `[icon=sprites/key.png]`
    /// (`renderer::rich_text`, Oct 2026). Off by default, so a string with
    /// brackets in it draws as it always did.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rich: bool,
    /// A typewriter: characters a second, waiting at each `[pause]`, from
    /// whenever `text` last changed. 0 (the default) shows it all at once.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub reveal_speed: f32,
    /// The typewriter's clock, kept by `rich_text::tick_reveals`; `None`
    /// until it first ticks, which shows the whole text.
    #[serde(skip)]
    pub reveal: Option<TextReveal>,
    /// An outline round every glyph ([`TextOutline`], Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<TextOutline>,
    /// A shadow under the text, offset and softened ([`TextShadow`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<TextShadow>,
    /// A string-table key (`locale`, Oct 2026): `text` is written from it in
    /// the current language, and again whenever that changes. Empty (the
    /// default) leaves `text` alone.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text_key: String,
    /// The `locale::stamp` the text was last written for. Never saved.
    #[serde(skip)]
    pub localized: u64,
}

/// An outline round world text (Oct 2026): the glyphs drawn again under
/// themselves, grown by `width` world units, in `color`. On a distance-field
/// font - the built-in one, or any `.font` made with `jengine font-sdf` -
/// it is the same glyph with its edge moved out, so it is round at the
/// corners and costs one more quad a glyph; a field encodes only so far past
/// the ink (two layout pixels of the built-in font, times its scale), and a
/// wider outline is cut to that. On a bitmap font it is eight offset copies.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextOutline {
    /// How far past the glyph's edge, in world units.
    pub width: f32,
    /// Linear RGBA.
    #[serde(default = "default_outline_color")]
    pub color: [f32; 4],
}

/// A shadow under world text (Oct 2026): the glyphs drawn again first,
/// moved by `offset` (world units, y up) in `color`, and on a distance-field
/// font blurred by `softness` world units - up to what the field encodes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextShadow {
    #[serde(default = "default_shadow_offset_text")]
    pub offset: [f32; 2],
    #[serde(default = "default_text_shadow_color")]
    pub color: [f32; 4],
    #[serde(default)]
    pub softness: f32,
}

fn default_outline_color() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

fn default_shadow_offset_text() -> [f32; 2] {
    [2.0, -2.0]
}

fn default_text_shadow_color() -> [f32; 4] {
    [0.0, 0.0, 0.0, 0.6]
}

/// Where a typewriter is (`TextComponent::reveal_speed`): seconds since the
/// text with this hash was set.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextReveal {
    pub key: u64,
    pub elapsed: f32,
}

fn default_font_scale() -> f32 {
    1.0
}

fn default_text_color() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

impl Default for TextComponent {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_id: String::new(),
            font_scale: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            z_order: 0,
            sort_layer: None,
            wrap_width: 0.0,
            align: TextAlign::Left,
            rich: false,
            reveal_speed: 0.0,
            reveal: None,
            outline: None,
            shadow: None,
            text_key: String::new(),
            localized: 0,
        }
    }
}

/// Dynamic custom components — stores arbitrary user-defined component data.
/// Keys are component names (e.g. "Health"), values are JSON objects with fields.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomComponents(pub HashMap<String, serde_json::Value>);

/// Tags for entity grouping — scripts can query entities by tag.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tags(pub std::collections::HashSet<String>);

/// Marks an entity as an instance of a named prefab template.
///
/// 2.2 — Prefab variants / overrides. Authored in a scene as
/// `"prefab_instance": { "prefab_id": "Player", "overrides": { ... } }`.
/// At scene-load time, `scene::prefab::expand_prefab_instances` looks
/// the template up in the runtime's prefab map, merges its components
/// onto the entity (any components the scene already authored win
/// over the template), then applies dotted-path overrides on top.
///
/// The component is retained on the spawned entity as a marker so
/// scripts / tooling can ask "what prefab did this come from?".
///
/// **A prefab can carry one too** (Oct 2026): it is then a *variant* of the
/// prefab it names - the base, its own components over it, then these
/// overrides (`scene::prefab::resolve_prefab_variants`, which every runtime
/// runs on its prefab map) - so a family of creatures is one prefab and its
/// differences, and a variant of a variant works.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrefabInstance {
    /// Name of the prefab template to instantiate. Looked up in the
    /// runtime's `services.prefabs` map (populated from
    /// `assets/prefabs/*.prefab`).
    pub prefab_id: String,
    /// Per-instance property overrides keyed by dotted path
    /// (e.g. `"sprite.color"`, `"transform.position"`, or a top-level
    /// component key like `"sprite"` to replace the whole component).
    /// Values are raw `serde_json::Value`s applied via
    /// `serde_json::Value::pointer_mut`-style traversal.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub overrides: HashMap<String, serde_json::Value>,
}

/// Camera follow component — makes the camera track this entity, or **frame
/// a whole set of them**.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraFollow {
    /// How fast the camera closes on its target, as a **fraction of the
    /// remaining distance per frame at 60fps**. Typical: 0.1-0.3.
    ///
    /// So **larger is faster**: `1.0` snaps instantly and `0.0` never moves
    /// at all. This doc comment used to say the opposite - "0 = instant
    /// snap, 1 = very slow follow" - which is exactly backwards, and the
    /// kind of error that costs an afternoon: a scene authored against it
    /// gets a camera that does not move and reads as a broken follow rather
    /// than a mistuned one. The arithmetic
    /// (`1 - (1 - smoothing)^(dt * 60)`) has always worked this way.
    #[serde(default = "default_smoothing")]
    pub smoothing: f32,
    /// Camera offset from the entity position (in world units).
    #[serde(default)]
    pub offset: Vec2,
    /// Optional bounds: [min_x, min_y, max_x, max_y]. Camera won't move beyond these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[f32; 4]>,

    /// Frame **every entity carrying this tag** instead of following the one
    /// this component is on: centre on their bounding box and zoom out until
    /// it fits.
    ///
    /// Following one entity is the wrong shape for a whole genre. A
    /// two-player duel, a co-op screen, a boss and the player who is
    /// running from it - each has a *set* of things that must all stay
    /// visible, and none of them is served by picking one to centre on. The
    /// arcade needed it for a ship melee, where both ships are constantly
    /// running away from each other.
    ///
    /// With no tagged entity in the world it falls back to following this
    /// entity, so a scene does not go blank when the things it was framing
    /// are destroyed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_tag: Option<String>,

    /// World units of margin around the framed set, so the things being
    /// framed are not flush against the edge of the screen.
    #[serde(default = "default_frame_padding")]
    pub frame_padding: f32,

    /// `[min, max]` zoom while framing. Zoom here is the renderer's: **a
    /// larger number is closer in**, so `min` is how far out the camera may
    /// pull and `max` is how far in it may push.
    ///
    /// The max matters more than it looks. Without a ceiling, two ships
    /// drifting together zoom the camera until the screen is two sprites
    /// and a wall of empty space, which is disorienting and hides
    /// everything either of them is about to run into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_range: Option<[f32; 2]>,

    /// A box round the middle of the view, `[width, height]` in world units,
    /// inside which the target moves without the camera following (Oct
    /// 2026; Godot's drag margins, Cinemachine's dead zone). Past its edge
    /// the camera is dragged along so the target sits on the edge. `[0, 0]`
    /// (the default) follows every step, as before.
    #[serde(default, skip_serializing_if = "is_zero_pair")]
    pub dead_zone: [f32; 2],
    /// Look ahead: aim this many **seconds** ahead of where the target is
    /// going, by its velocity - so a runner sees what it runs into rather
    /// than what it left. 0 (the default) is off.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub look_ahead: f32,
    /// The furthest the look-ahead may lead, in world units.
    #[serde(default = "default_look_ahead_max")]
    pub look_ahead_max: f32,
    /// How fast the look-ahead swings round when the target turns: a
    /// fraction of the way per frame at 60 fps, like `smoothing`. Slow, so a
    /// turn does not throw the view.
    #[serde(default = "default_look_ahead_smoothing")]
    pub look_ahead_smoothing: f32,
}

fn is_zero_pair(v: &[f32; 2]) -> bool {
    v[0] == 0.0 && v[1] == 0.0
}

fn default_look_ahead_max() -> f32 {
    160.0
}

fn default_look_ahead_smoothing() -> f32 {
    0.04
}

fn default_smoothing() -> f32 {
    0.1
}

/// A fifth of a screen's worth at the arcade's usual size. Enough that a
/// framed thing is not flush against the edge, small enough that the view
/// does not feel padded out.
fn default_frame_padding() -> f32 {
    140.0
}

impl Default for CameraFollow {
    fn default() -> Self {
        Self {
            smoothing: 0.1,
            offset: Vec2::ZERO,
            bounds: None,
            frame_tag: None,
            frame_padding: default_frame_padding(),
            zoom_range: None,
            dead_zone: [0.0, 0.0],
            look_ahead: 0.0,
            look_ahead_max: default_look_ahead_max(),
            look_ahead_smoothing: default_look_ahead_smoothing(),
        }
    }
}

/// `Camera.set_follow(e, field, value)`: any field of the entity's
/// [`CameraFollow`] by name - a cutscene loosening the follow, a dead zone
/// switched on for a platforming stretch.
pub fn set_follow_field(world: &hecs::World, entity: hecs::Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut f = world.get::<&mut CameraFollow>(entity).map_err(|_| "the entity has no camera_follow".to_string())?;
    let mut whole = serde_json::to_value(&*f).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    let known = ["dead_zone", "look_ahead", "look_ahead_max", "look_ahead_smoothing", "frame_tag", "zoom_range", "bounds"];
    if !map.contains_key(field) && !known.contains(&field) {
        return Err(format!("'{field}' is not a camera_follow field"));
    }
    map.insert(field.to_string(), value.clone());
    *f = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    Ok(())
}

/// Camera component — marks an entity as a game camera.
///
/// Multiple cameras can exist: the active one with no `target` and the highest
/// `priority` draws the screen (`ecs::camera_rig::main_camera`), and one taking
/// over from another can `blend`. Requires a `Transform` on the same entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Camera2DComponent {
    /// Zoom level (1.0 = default). Higher values zoom in.
    #[serde(default = "default_cam_zoom")]
    pub zoom: f32,
    /// Whether this camera is the currently active (rendering) camera.
    #[serde(default)]
    pub is_active: bool,
    /// Viewport width hint (overridden at runtime by actual window size).
    #[serde(default = "default_cam_viewport_w")]
    pub viewport_width: f32,
    /// Viewport height hint (overridden at runtime by actual window size).
    #[serde(default = "default_cam_viewport_h")]
    pub viewport_height: f32,
    /// Stretch `viewport_width` x `viewport_height` over a screen of another
    /// shape, instead of showing more world along the longer axis
    /// (`Camera2D::fit_view`). Off by default: stretched, a square is drawn
    /// wide on a phone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stretch: bool,
    /// Draw the world at one pixel per `pixel_size` world units and scale it
    /// up - pixel art at its own resolution, on its own grid, scrolling
    /// smoothly (`renderer::pixel_perfect`). 0, the default, draws at the
    /// screen's resolution. Script: `Camera.set_pixel_size`.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub pixel_size: f32,
    /// With `pixel_size`, draw every art pixel as a whole number of screen
    /// pixels, showing a little more world to fill the screen
    /// (`Camera2D::integer_view`). Off by default: the authored area fills
    /// the screen exactly and an art pixel may be 4.27 screen pixels wide.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub integer_scale: bool,
    /// Draw into a texture of this name instead of onto the screen: a
    /// **view** (`ecs::views`). Any sprite, mesh or UI image naming it as
    /// its `texture_id` shows what this camera sees - a monitor, a minimap,
    /// one half of a split screen. Empty, the default, is the main camera.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// The target texture's size in pixels; 0 takes the screen's.
    #[serde(default, skip_serializing_if = "is_zero_size")]
    pub target_size: [u32; 2],
    /// Which render layers this camera draws (a bitmask against each
    /// entity's `render_layers`, layer 1 for one with none). All of them by
    /// default - Unity's culling mask, Godot's cull mask: a minimap that
    /// shows markers the street views do not.
    #[serde(default = "all_layers", skip_serializing_if = "is_all_layers")]
    pub cull_mask: u32,
    /// Draw onto this part of the screen (Oct 2026; Unity's viewport rect):
    /// `[x, y, w, h]` as fractions of the screen from its top-left, y down -
    /// `[0, 0, 0.5, 1]` is the left half. The camera becomes a view drawing
    /// into a texture of its share's size, shown by an image beneath every
    /// other UI (`ecs::views::sync_screen_rects`) - split screen with no
    /// target or UI to author. All zero, the default, is none.
    #[serde(default, skip_serializing_if = "is_zero_rect")]
    pub screen_rect: [f32; 4],
    /// Which active camera draws the screen (Oct 2026; Cinemachine's
    /// priority): the highest, the first of them on a tie. Raise one to hand
    /// it the view - `Camera.set(e, "priority", 10)`.
    #[serde(default, skip_serializing_if = "is_zero_i32", deserialize_with = "i32_from_number")]
    pub priority: i32,
    /// Seconds the view takes to ease over to this camera when it takes
    /// over the screen from another, following its target the whole way. 0,
    /// the default, cuts.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub blend: f32,
    /// A polygon, in world units, this camera's **view** stays inside -
    /// every edge of the screen, not only its centre (Cinemachine's
    /// confiner): a gallery of any shape whose walls the view never looks
    /// past (`ecs::camera_rig::confine`). Empty, the default, is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confine: Vec<[f32; 2]>,
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

/// An integer that may arrive as a float - every number a Lua script hands
/// over is one natively (`10` is `10.0`), and serde refuses `10.0` for an
/// `i32`. Rounded.
fn i32_from_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    let v = f64::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("priority must be a number"));
    }
    Ok(v.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
}

fn is_zero_rect(v: &[f32; 4]) -> bool {
    *v == [0.0; 4]
}

fn all_layers() -> u32 {
    u32::MAX
}

fn is_all_layers(v: &u32) -> bool {
    *v == u32::MAX
}

/// Which render layers this entity's drawing is on, as a bitmask: a camera
/// draws it when its `cull_mask` shares a bit (`ecs::wgpu_systems::in_view_layers`).
/// An entity without one is on layer 1 (bit 0), which every camera draws
/// unless told not to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RenderLayers {
    pub layers: u32,
}

impl Default for RenderLayers {
    fn default() -> Self {
        Self { layers: 1 }
    }
}

/// The frame's **colour grade** as a look-up table (Oct 2026; Unity's Color
/// Lookup): a strip of `n` slices `n` x `n` - red right, green down, blue
/// slice by slice - painted by grading a screenshot with the neutral strip
/// (`renderer::lut::identity`) on it. Applied in the output pass after the
/// tonemap, mixed in by `strength`. The first one in the world is used.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ColorLut {
    /// The strip's texture.
    pub texture_id: String,
    /// 0 is no grade, 1 the LUT's.
    pub strength: f32,
}

impl Default for ColorLut {
    fn default() -> Self {
        Self { texture_id: Default::default(), strength: 1.0 }
    }
}

/// Which **light layers** an entity's sprite is on, as a bitmask (Oct 2026;
/// Godot's `light_mask`, Unity's sorting layers as a light's targets): a
/// point, spot or directional light lights it only when the light's
/// `light_mask` shares a bit. A sprite without one is on layer 1 (bit 0),
/// which every light lights unless told not to. Eight layers. The ambient
/// light, lightmaps and freeform lights light every layer, and so is
/// everything that is not a sprite - tilemaps, text, meshes, particles.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LightLayers {
    /// The layers, as bits: layer 1 is bit 0 (`1`), layer 2 bit 1 (`2`), and
    /// a sprite on both is `3`.
    pub layers: u32,
}

impl Default for LightLayers {
    fn default() -> Self {
        Self { layers: 1 }
    }
}

/// A mask: what entities with `masked` naming this one are clipped to - a
/// portal, a window, a frame (`renderer::mask`). The shape is in this
/// entity's space, so it moves, turns and scales with its transform.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SpriteMask {
    pub shape: crate::renderer::mask::MaskShape,
    /// The soft edge, in the shape's own units; 0 is hard.
    pub feather: f32,
}

/// Clips this entity's drawing - its sprite, mesh, path, world text, or its
/// emitter's particles - to the `sprite_mask` on the entity named `by`; with
/// `outside`, to everything but it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Masked {
    pub by: String,
    pub outside: bool,
}

fn is_zero_size(v: &[u32; 2]) -> bool {
    *v == [0, 0]
}

fn default_cam_zoom() -> f32 { 1.0 }
fn default_cam_viewport_w() -> f32 { 1280.0 }
fn default_cam_viewport_h() -> f32 { 720.0 }

impl Camera2DComponent {
    /// How much world this camera shows on a `surface` of that many pixels:
    /// its authored area fitted to the surface's shape
    /// (`Camera2D::fit_view`), then widened to whole screen pixels per art
    /// pixel when `integer_scale` asks (`Camera2D::integer_view`). The one
    /// rule the projection, the mouse and every host's published camera use.
    pub fn view_on(&self, surface: glam::Vec2) -> glam::Vec2 {
        use crate::renderer::camera::Camera2D;
        // A camera drawing onto part of the screen authors that part of the
        // area: half the screen shows half the world at the same scale, as
        // Unity's viewport rect does, rather than the whole of it shrunk.
        let share = if self.screen_rect[2] > 0.0 && self.screen_rect[3] > 0.0 {
            glam::Vec2::new(self.screen_rect[2], self.screen_rect[3])
        } else {
            glam::Vec2::ONE
        };
        let authored = glam::Vec2::new(self.viewport_width, self.viewport_height) * share;
        let view = Camera2D::fit_view(authored, surface, self.stretch);
        if self.integer_scale && !self.stretch {
            Camera2D::integer_view(view, surface, self.pixel_size, self.zoom)
        } else {
            view
        }
    }
}

impl Default for Camera2DComponent {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            is_active: true,
            viewport_width: 1280.0,
            viewport_height: 720.0,
            stretch: false,
            pixel_size: 0.0,
            integer_scale: false,
            target: String::new(),
            target_size: [0, 0],
            cull_mask: u32::MAX,
            screen_rect: [0.0; 4],
            priority: 0,
            blend: 0.0,
            confine: Vec::new(),
        }
    }
}

// -- Phase 4B: Physics --

/// Rigid body type for physics simulation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BodyType {
    Dynamic,
    Static,
    Kinematic,
}

impl BodyType {
    /// Parse the name a script passes to `set_body_type`.
    ///
    /// # Why this is not three `match` arms at three call sites
    ///
    /// It was, and they disagreed. The native runtime and the web host matched
    /// `"static"` and `"kinematic"` only; the editor viewport also accepted
    /// `"Static"` and `"Kinematic"`. So a script written and tested in the
    /// editor with `set_body_type(e, "Static")` fell through to `Dynamic` in
    /// the shipped game — no error, no log, just a body that behaved
    /// completely differently. Nothing could catch that: the tag was
    /// dispatched by both hosts, so every parity guard passed.
    ///
    /// Case-insensitive, because that is the superset: no script that worked
    /// anywhere stops working, and the trap disappears.
    ///
    /// Returns `None` for an unrecognised name so the caller can say so.
    /// Callers still fall back to [`BodyType::Dynamic`], which is the
    /// long-standing behaviour, but a typo now leaves a log line rather than
    /// silently turning a wall into a falling object.
    pub fn from_script_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "static" => Some(Self::Static),
            "kinematic" => Some(Self::Kinematic),
            "dynamic" => Some(Self::Dynamic),
            _ => None,
        }
    }
}

/// Rigid body component — attaches a physics body to an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigidBody2D {
    pub body_type: BodyType,
    #[serde(default = "default_mass")]
    pub mass: f32,
    #[serde(default = "default_gravity_scale")]
    pub gravity_scale: f32,
    #[serde(default)]
    pub linear_damping: f32,
    #[serde(default)]
    pub angular_damping: f32,
    #[serde(default)]
    pub fixed_rotation: bool,
    /// Sweep this body's shape along its path instead of only testing
    /// where it landed - **continuous collision detection**.
    ///
    /// Rapier integrates velocity and then looks for overlaps, so a body
    /// that crosses more than its own thickness in one step can be past
    /// a wall by the time anything is measured: no contact is generated
    /// and it simply leaves. A 12-unit ball at 3000 units a second
    /// covers 50 units in a 60 Hz step.
    ///
    /// The engine has always constructed a `CCDSolver` and handed it to
    /// every step, but rapier only sweeps bodies that ask - so until this
    /// field existed the solver ran on an empty set for the life of the
    /// engine, and nothing could have used it. No game here had gone fast
    /// enough to notice.
    ///
    /// Off by default, and deliberately: the sweep is not free, and a
    /// scene that does not mention it behaves exactly as before. Turn it
    /// on for what is small and fast - a pinball, a bullet, a thrown
    /// weapon - and leave it off for crates and characters.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ccd: bool,
    /// Whether rapier may put this body to sleep when it stops moving.
    ///
    /// Sleeping is right for almost everything: a crate on a floor
    /// should cost nothing once it has settled. It is wrong for anything
    /// that is *supposed* to keep moving. A pinball that comes to rest
    /// for an instant on a steep slope is put to sleep there and stays -
    /// gravity does not wake a sleeping body, so it sits on a 53-degree
    /// ramp with a friction coefficient of 0.03 and never slides, which
    /// reads as the ball being glued to the wall.
    ///
    /// Defaults to true, which is rapier's own default and the previous
    /// behaviour of every body in the engine.
    #[serde(default = "default_true_sleep", skip_serializing_if = "std::clone::Clone::clone")]
    pub can_sleep: bool,
}

fn default_true_sleep() -> bool {
    true
}

fn default_mass() -> f32 {
    1.0
}

fn default_gravity_scale() -> f32 {
    1.0
}

impl Default for RigidBody2D {
    fn default() -> Self {
        Self {
            body_type: BodyType::Dynamic,
            mass: 1.0,
            gravity_scale: 1.0,
            linear_damping: 0.0,
            angular_damping: 0.0,
            fixed_rotation: false,
            ccd: false,
            can_sleep: true,
        }
    }
}

/// Shape type for a 2D collider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColliderShape {
    Box { width: f32, height: f32 },
    Circle { radius: f32 },
    Capsule { half_height: f32, radius: f32 },
    Segment { a: [f32; 2], b: [f32; 2] },
    Triangle { a: [f32; 2], b: [f32; 2], c: [f32; 2] },
    ConvexPolygon { vertices: Vec<[f32; 2]> },
    Polyline { vertices: Vec<[f32; 2]> },
    HeightField { heights: Vec<f32>, scale: [f32; 2] },
    /// A concave polygon cut into convex pieces. `loops` holds the index in
    /// `vertices` where each loop after the first starts (Oct 2026), so one
    /// shape can be several islands - a sprite's outline traced by
    /// `sprite_collider`. Empty is the one loop it always was.
    ConvexDecomposition {
        vertices: Vec<[f32; 2]>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        loops: Vec<u32>,
    },
}

/// The loops of a [`ColliderShape::ConvexDecomposition`]: `vertices` cut at
/// each start in `loops` (out-of-order or out-of-range starts are ignored).
pub fn decomposition_loops<'a>(vertices: &'a [[f32; 2]], loops: &[u32]) -> Vec<&'a [[f32; 2]]> {
    let mut out = Vec::new();
    let mut from = 0usize;
    for &start in loops {
        let start = start as usize;
        if start > from && start < vertices.len() {
            out.push(&vertices[from..start]);
            from = start;
        }
    }
    out.push(&vertices[from..]);
    out
}

/// Collider component — defines the collision shape for an entity's body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collider2D {
    pub shape: ColliderShape,
    #[serde(default)]
    pub offset: [f32; 2],
    #[serde(default)]
    pub is_sensor: bool,
    #[serde(default = "default_friction")]
    pub friction: f32,
    #[serde(default = "default_restitution")]
    pub restitution: f32,
    #[serde(default = "default_density")]
    pub density: f32,
    /// Collision layer membership bitmask — which layers this collider belongs to.
    #[serde(default = "default_collision_all")]
    pub collision_membership: u16,
    /// Collision layer filter bitmask — which layers this collider can collide with.
    #[serde(default = "default_collision_all")]
    pub collision_filter: u16,
    /// A platform a [`CharacterController2D`] can jump up through and stand
    /// on: it blocks a character only from above, and one that asks to
    /// `drop_through` falls through it. Other bodies collide with it as
    /// usual.
    #[serde(default)]
    pub one_way: bool,
}

/// Character controller: moves a **kinematic** body the way a platformer or a
/// top-down hero moves - by a velocity it asks for each frame, stopped by
/// walls, sliding along them, climbing slopes up to `max_slope_deg`, stepping
/// up ledges up to `step_height`, staying on the ground down slopes
/// (`snap_to_ground`), carried by the kinematic platform it stands on, and
/// standing on one-way platforms.
///
/// It applies no gravity and no acceleration of its own: that is the game's
/// feel, and belongs to the script or package setting `velocity` (a
/// platformer package's hero module, say). The entity needs a kinematic `RigidBody2D`
/// and a `Collider2D`; a capsule or a box with rounded corners slides best.
///
/// The physics step fills in the rest of the fields after it moves the body.
/// Scripts set the velocity with `move_character` and read the result with
/// `get_character`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterController2D {
    /// The movement asked for this frame, in units per second.
    pub velocity: [f32; 2],
    /// The gap kept between the character and what it touches, in units.
    pub skin: f32,
    /// The steepest slope it walks up, in degrees; anything steeper is a wall.
    pub max_slope_deg: f32,
    /// Slopes at least this steep slide it down, in degrees.
    pub min_slide_deg: f32,
    /// How far down it looks for ground to stay on while walking down a
    /// slope or off a small step, in units. 0 turns it off.
    pub snap_to_ground: f32,
    /// The tallest ledge it steps up without jumping, in units. 0 turns it off.
    pub step_height: f32,
    /// The least room a ledge must have on top to be stepped onto, in units.
    pub step_width: f32,
    /// Pushes dynamic bodies it walks into as if it weighed this much. 0
    /// leaves them alone (they still block it).
    pub push_mass: f32,
    /// Falls through one-way platforms while set.
    pub drop_through: bool,
    /// Goes down with a kinematic platform it stands on. (Being carried
    /// sideways and upward is rapier's own doing and always happens.)
    pub ride_platforms: bool,

    // ---- Written by the physics step ----
    /// Standing on something.
    #[serde(skip)]
    pub grounded: bool,
    /// The normal of what it stands on (0, 1 on flat ground).
    #[serde(skip)]
    pub ground_normal: [f32; 2],
    /// The entity it stands on.
    #[serde(skip)]
    pub ground_entity: Option<u64>,
    /// -1 against a wall on its left, 1 on its right, 0 for none.
    #[serde(skip)]
    pub wall: i32,
    /// Bumped its head this frame.
    #[serde(skip)]
    pub ceiling: bool,
    /// Sliding down a slope too steep to stand on.
    #[serde(skip)]
    pub sliding: bool,
    /// How it actually moved this frame, in units per second: `velocity`
    /// less what was blocked, plus what a platform carried.
    #[serde(skip)]
    pub moved: [f32; 2],
    /// Every entity whose collider it overlaps or touches after moving,
    /// sensors included - pickups, hazards, doors.
    #[serde(skip)]
    pub touching: Vec<u64>,
}

/// Tilemap layers whose tiles cast shadows (Oct 2026; Godot's TileSet
/// occlusion layers, Unity's TilemapShadowCaster).
///
/// Each tile casts its tileset's shape for it ([`crate::tilemap::TileShape`]:
/// its occluder, else its collision outline, else the whole cell), and the
/// cells are merged into a few long outlines before the lights see them
/// ([`crate::tilemap::occluder_loops`]) - a wall of a hundred tiles is the
/// four edges round it, not four hundred. The rest is a `light_occluder`'s:
/// the same tint, opacity, `self_shadow` (off: rock lit by a lamp in the cave
/// is lit itself, rock beyond it is in shadow), `shadows_casters` and
/// `shadow_length`, applied to the whole map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilemapOccluder {
    /// When false the map casts no shadow.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The layers that cast shadows, by name.
    #[serde(default)]
    pub layers: Vec<String>,
    /// As [`LightOccluder::shadow_tint`].
    #[serde(default = "default_shadow_tint")]
    pub shadow_tint: [f32; 3],
    /// As [`LightOccluder::shadow_opacity`].
    #[serde(default = "default_shadow_opacity")]
    pub shadow_opacity: f32,
    /// As [`LightOccluder::self_shadow`].
    #[serde(default)]
    pub self_shadow: bool,
    /// As [`LightOccluder::shadows_casters`].
    #[serde(default = "default_true_occ", skip_serializing_if = "is_true_occ")]
    pub shadows_casters: bool,
    /// As [`LightOccluder::shadow_length`].
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub shadow_length: f32,
    /// The merged outlines, kept until the tiles or shapes change. Never saved.
    #[serde(skip)]
    pub cache: crate::tilemap::shapes::OutlineCache,
}

impl Default for TilemapOccluder {
    fn default() -> Self {
        Self {
            enabled: true,
            layers: Vec::new(),
            shadow_tint: default_shadow_tint(),
            shadow_opacity: default_shadow_opacity(),
            self_shadow: false,
            shadows_casters: true,
            shadow_length: 0.0,
            cache: Default::default(),
        }
    }
}

impl TilemapOccluder {
    /// The `light_occluder` each outline is cast as.
    pub fn as_light_occluder(&self) -> LightOccluder {
        LightOccluder {
            enabled: self.enabled,
            shadow_tint: self.shadow_tint,
            shadow_opacity: self.shadow_opacity,
            self_shadow: self.self_shadow,
            shadows_casters: self.shadows_casters,
            shadow_length: self.shadow_length,
            ..serde_json::from_str("{}").expect("a light_occluder has every default")
        }
    }
}

/// Tilemap collision: which layers of the entity's `Tilemap` are solid
/// ground and which are one-way platforms, by layer name.
///
/// The physics step builds one static body for the tilemap from its tiles,
/// merged into as few boxes as it can, and rebuilds it when those tiles
/// change (a block broken with `tilemap_set_tile`). A layer flagged
/// `collision` (Tiled names it so) is solid whether it is listed or not, and
/// a tilemap with such a layer collides even without this component.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TilemapCollider {
    /// Layers whose every tile is solid.
    pub solid: Vec<String>,
    /// Layers whose tiles are one-way platforms (see `Collider2D::one_way`).
    pub one_way: Vec<String>,
}

impl Default for CharacterController2D {
    fn default() -> Self {
        Self {
            velocity: [0.0, 0.0],
            skin: 0.5,
            max_slope_deg: 50.0,
            min_slide_deg: 55.0,
            snap_to_ground: 8.0,
            step_height: 0.0,
            step_width: 4.0,
            push_mass: 0.0,
            drop_through: false,
            ride_platforms: true,
            grounded: false,
            ground_normal: [0.0, 1.0],
            ground_entity: None,
            wall: 0,
            ceiling: false,
            sliding: false,
            moved: [0.0, 0.0],
            touching: Vec::new(),
        }
    }
}

fn default_friction() -> f32 {
    0.5
}

fn default_restitution() -> f32 {
    0.0
}

fn default_density() -> f32 {
    1.0
}

fn default_collision_all() -> u16 {
    0xFFFF
}

impl Default for Collider2D {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Box {
                width: 1.0,
                height: 1.0,
            },
            offset: [0.0, 0.0],
            is_sensor: false,
            friction: 0.5,
            restitution: 0.0,
            density: 1.0,
            collision_membership: 0xFFFF,
            collision_filter: 0xFFFF,
            one_way: false,
        }
    }
}

// -- Joints --

/// Joint type for connecting two physics bodies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JointType {
    Revolute {
        /// Where on **this** entity's body the pivot sits, in its local
        /// frame. Usually `[0, 0]`: the thing that turns, turns about
        /// its own centre.
        anchor: [f32; 2],
        /// Where on the **target** body the pivot sits, in its local
        /// frame. This is the mount point - the back of a chassis, the
        /// top of a post.
        ///
        /// It defaults to the target's origin, which is what this joint
        /// always used to do and could not be talked out of. That made
        /// two wheels on one chassis impossible to express: both pinned
        /// to the same point. See
        /// `tests/joints_mount_where_they_are_told.rs`.
        #[serde(default)]
        anchor_b: [f32; 2],
        limits: Option<[f32; 2]>,
        #[serde(default)]
        motor_speed: f32,
        #[serde(default)]
        motor_max_torque: f32,
    },
    Fixed {
        anchor_a: [f32; 2],
        anchor_b: [f32; 2],
    },
    Prismatic {
        anchor: [f32; 2],
        /// The mount point on the target body. Defaults to its origin,
        /// as this joint always behaved. See `Revolute::anchor_b`.
        #[serde(default)]
        anchor_b: [f32; 2],
        axis: [f32; 2],
        limits: Option<[f32; 2]>,
        #[serde(default)]
        motor_speed: f32,
        #[serde(default)]
        motor_max_force: f32,
    },
    Rope {
        anchor_a: [f32; 2],
        anchor_b: [f32; 2],
        max_distance: f32,
    },
    Spring {
        anchor_a: [f32; 2],
        anchor_b: [f32; 2],
        rest_length: f32,
        stiffness: f32,
        damping: f32,
    },
    /// A wheel on a suspension (Oct 2026; Unity's `WheelJoint2D`, Box2D's
    /// wheel joint). This entity is the wheel and the target the chassis:
    /// the wheel slides along `axis` - in the chassis's frame, up by default
    /// - against a spring, and turns freely or as its motor drives it. A
    /// revolute wheel is pinned rigidly, so its car takes every bump in the
    /// frame.
    ///
    /// The spring is a frequency and a damping ratio, as Unity's is, and does
    /// not depend on the masses: 2 Hz bounces twice a second whatever it
    /// carries. The motor turns the wheel against the chassis, and a positive
    /// `motor_speed` turns it clockwise - rolling a car right, as a revolute
    /// wheel's does.
    Wheel {
        /// The axle on this entity, in its frame - usually its centre.
        #[serde(default)]
        anchor: [f32; 2],
        /// Where the axle hangs on the chassis, in the chassis's frame.
        #[serde(default)]
        anchor_b: [f32; 2],
        /// The suspension's direction, in the chassis's frame.
        #[serde(default = "wheel_axis")]
        axis: [f32; 2],
        /// How stiff the suspension is: the frequency it bounces at, in Hz.
        #[serde(default = "wheel_frequency")]
        frequency: f32,
        /// 0 bounces for ever, 1 settles without overshooting.
        #[serde(default = "wheel_damping_ratio")]
        damping_ratio: f32,
        /// How far the wheel may travel along the axis, `[min, max]`, from
        /// where the anchors meet.
        #[serde(default)]
        limits: Option<[f32; 2]>,
        #[serde(default)]
        motor_speed: f32,
        #[serde(default)]
        motor_max_torque: f32,
    },
    /// Holds this entity at an offset from the target with limited strength
    /// (Oct 2026; Unity's `RelativeJoint2D`): `linear_offset` in the
    /// target's frame and `angular_offset` from its angle, pulled toward by a
    /// spring of `frequency` and `damping_ratio` that never pushes harder
    /// than `max_force` or turns harder than `max_torque` (0 is no limit). A
    /// turret that follows its hull, a load that sways back into place.
    Motor {
        #[serde(default)]
        linear_offset: [f32; 2],
        #[serde(default)]
        angular_offset: f32,
        #[serde(default)]
        max_force: f32,
        #[serde(default)]
        max_torque: f32,
        #[serde(default = "motor_joint_frequency")]
        frequency: f32,
        #[serde(default = "one_f32")]
        damping_ratio: f32,
    },
    /// Friction against the target (Oct 2026; Unity's `FrictionJoint2D`):
    /// sliding and turning relative to it are resisted by at most
    /// `max_force` and `max_torque`. Top-down games have no floor to rub on,
    /// so this is how a puck slows, a crate drags and a hook's trolley
    /// sticks.
    Friction {
        #[serde(default)]
        anchor_a: [f32; 2],
        #[serde(default)]
        anchor_b: [f32; 2],
        #[serde(default)]
        max_force: f32,
        #[serde(default)]
        max_torque: f32,
    },
    /// Pulls a point of this entity toward a point in the world (Oct 2026;
    /// Unity's `TargetJoint2D`) - dragging with the mouse. The point is
    /// where the joint's `target_entity` is, every step, or `target` when
    /// it names none; the pull is a spring of `frequency` and
    /// `damping_ratio` that never exceeds `max_force` (0 is no limit), so a
    /// heavy thing lags and a light one snaps to it. There is no second
    /// body: the joint holds a kinematic anchor of its own.
    Target {
        /// The point that is pulled, in this entity's frame.
        #[serde(default)]
        anchor: [f32; 2],
        /// Where it is pulled to when `target_entity` names nothing.
        #[serde(default)]
        target: [f32; 2],
        #[serde(default = "target_joint_frequency")]
        frequency: f32,
        #[serde(default = "target_joint_damping_ratio")]
        damping_ratio: f32,
        #[serde(default)]
        max_force: f32,
    },
}

fn wheel_axis() -> [f32; 2] {
    [0.0, 1.0]
}
fn wheel_frequency() -> f32 {
    3.0
}
fn wheel_damping_ratio() -> f32 {
    0.7
}
fn motor_joint_frequency() -> f32 {
    5.0
}
fn one_f32() -> f32 {
    1.0
}
fn target_joint_frequency() -> f32 {
    5.0
}
fn target_joint_damping_ratio() -> f32 {
    0.7
}

/// Joint component — connects this entity's body to another entity's body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Joint2D {
    /// Name of the target entity to connect to.
    pub target_entity: String,
    /// The joint type and its parameters.
    pub joint_type: JointType,
    /// Whether connected bodies can collide with each other (default: false).
    #[serde(default)]
    pub collide_connected: bool,
    /// The force that snaps the joint (Oct 2026; Unity's `breakForce`), in
    /// the world's units - mass times units a second squared, so a body of
    /// mass `m` hanging still pulls `m * 981` in a project at the usual
    /// `length_unit`. 0, the default, never breaks. A broken joint is gone:
    /// the `joint` component is removed and the entity's script hears
    /// `on_joint_break`.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub break_force: f32,
    /// The torque that snaps a hinge, likewise: what its **limit** (and its
    /// motor) carries - a door held open on its stop, a ragdoll limb bent
    /// past its swing. A free hinge carries none; the angle a fixed joint
    /// locks is not measured. 0 never breaks.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub break_torque: f32,
}

impl Default for Joint2D {
    fn default() -> Self {
        Self {
            target_entity: String::new(),
            joint_type: JointType::Revolute {
                anchor: [0.0, 0.0],
                anchor_b: [0.0, 0.0],
                limits: None,
                motor_speed: 0.0,
                motor_max_torque: 0.0,
            },
            collide_connected: false,
            break_force: 0.0,
            break_torque: 0.0,
        }
    }
}

// -- Phase 4A: Audio --

/// Audio source component — attaches a sound clip to an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSource {
    /// Asset path for the audio clip (e.g. "sounds/jump.ogg").
    pub clip_id: String,
    /// Volume (0.0 = silent, 1.0 = full).
    #[serde(default = "default_audio_volume")]
    pub volume: f32,
    /// Pitch multiplier (1.0 = normal).
    #[serde(default = "default_audio_pitch")]
    pub pitch: f32,
    /// Whether this clip loops.
    #[serde(default)]
    pub looping: bool,
    /// Set once the source has been started, so autoplay fires exactly once.
    /// Runtime state; never serialized.
    #[serde(default, skip)]
    pub started: bool,
    /// If true, start playing automatically when the entity spawns.
    #[serde(default)]
    pub auto_play: bool,
    /// 2.4 — mixer bus this source plays through. `None` routes to
    /// the default `sfx` bus. Custom buses can be created at runtime
    /// via `audio_create_bus(name, parent, volume)`.
    #[serde(default)]
    pub bus_id: Option<String>,
    /// Heard from where it is (Oct 2026, `audio::spatial`): louder near the
    /// listener - an `audio_listener`, or the active camera - and panned to
    /// its side, every frame as either moves. Off, it plays as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spatial: bool,
    /// Full volume within this distance of the listener.
    #[serde(default = "default_min_distance")]
    pub min_distance: f32,
    /// Silent from this distance.
    #[serde(default = "default_max_distance")]
    pub max_distance: f32,
    /// A source this far to one side is panned all the way to it.
    #[serde(default = "default_pan_width")]
    pub pan_width: f32,
    /// The share of its volume each wall between it and the listener takes
    /// (`audio::spatial`; 0: walls change nothing). A wall is any non-sensor
    /// collider the line between them crosses.
    #[serde(default)]
    pub occlusion: f32,
    /// Its low-pass cutoff behind one wall, in hertz - each further wall
    /// halves it (0: no muffle).
    #[serde(default)]
    pub muffle: f32,
}

fn default_min_distance() -> f32 {
    crate::audio::spatial::DEFAULT_MIN_DISTANCE
}

fn default_max_distance() -> f32 {
    crate::audio::spatial::DEFAULT_MAX_DISTANCE
}

fn default_pan_width() -> f32 {
    crate::audio::spatial::DEFAULT_PAN_WIDTH
}

fn default_audio_volume() -> f32 {
    1.0
}

fn default_audio_pitch() -> f32 {
    1.0
}

impl Default for AudioSource {
    fn default() -> Self {
        Self {
            clip_id: String::new(),
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            auto_play: false,
            started: false,
            bus_id: None,
            spatial: false,
            min_distance: default_min_distance(),
            max_distance: default_max_distance(),
            pan_width: default_pan_width(),
            occlusion: 0.0,
            muffle: 0.0,
        }
    }
}

// -- Phase 5: GUI --

/// GUI component — attaches a `.gui` layout document to an entity.
///
/// At runtime the layout engine reads the referenced `GuiDocument` from the
/// asset manager, resolves the widget tree into screen-space rectangles, and
/// renders them on top of the game world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiComponent {
    /// Asset ID of the `.gui` document (e.g. "ui/hud.gui").
    pub gui_id: String,
    /// Whether this GUI is currently visible.
    #[serde(default = "default_gui_visible")]
    pub visible: bool,
    /// Dynamic variables that scripts can bind to GUI nodes
    /// (e.g. `{ "score": 42, "health": 0.75 }`).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub variables: HashMap<String, serde_json::Value>,
    /// ID of the currently focused node (for keyboard navigation).
    /// `None` means no node has focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_node_id: Option<String>,
    /// Runtime state - not serialized. The focus was put there by
    /// navigation (`navigate_ui`), not by a click: only then may A / Enter /
    /// Space press it. A click that happens to focus a button must not let a
    /// later Space - a game's jump, its "start wave" - press it again.
    #[serde(skip)]
    pub nav_focus: bool,
    /// Runtime state: the button under the pointer, set once a frame by
    /// `navigate_ui` in every runtime - what draws a button's `hover_color`.
    #[serde(skip)]
    pub hovered_node_id: Option<String>,
    /// Runtime state: the pointer is down over `hovered_node_id` - its
    /// `pressed_color`.
    #[serde(skip)]
    pub pointer_down: bool,
    /// Runtime state: how far into its hover `hovered_node_id` is, 0..1 -
    /// eased over [`GUI_HOVER_SECONDS`], so a button's colour and
    /// `hover_scale` come on smoothly rather than in one frame.
    #[serde(skip)]
    pub hover_t: f32,
    /// Runtime state: the pointer in UI pixels this frame, and a click on a
    /// `TextInput` waiting for `apply_text_input` to put the caret there
    /// (its x, whether shift was held) - `track_gui_pointer` sees the
    /// click, the text field's update owns the document.
    #[serde(skip)]
    pub pointer_ui: Option<(f32, f32)>,
    #[serde(skip)]
    pub text_click: Option<(f32, bool)>,
    /// Runtime state: a `Slider` the pointer pressed and still holds, and
    /// the value under the pointer this frame - `track_gui_pointer` sees the
    /// drag, `navigate_ui` (which owns the document) writes the value.
    #[serde(skip)]
    pub slider_drag: Option<(String, f32)>,
}

/// How long a `.gui` button takes to ease into its hover.
pub const GUI_HOVER_SECONDS: f32 = 0.12;

fn default_gui_visible() -> bool {
    true
}

impl Default for GuiComponent {
    fn default() -> Self {
        Self {
            gui_id: String::new(),
            visible: true,
            variables: HashMap::new(),
            focused_node_id: None,
            nav_focus: false,
            hovered_node_id: None,
            pointer_down: false,
            hover_t: 0.0,
            pointer_ui: None,
            text_click: None,
            slider_drag: None,
        }
    }
}

// -- Material --

use crate::renderer::material::{BlendMode, UniformValue};

/// Material component — assigns a named material to an entity for rendering.
///
/// When present on an entity with a `SpriteComponent`, the renderer will use the
/// referenced material (shader + blend mode + uniforms) instead of the default sprite
/// material. If the material is not found in the `MaterialLibrary`, the default is used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialComponent {
    /// Name of the material in the `MaterialLibrary` (e.g. "default_sprite", "additive").
    pub material_id: MaterialId,
    /// How the sprite blends with what is under it: `Alpha` (the default),
    /// `Additive` (glows, fire, light - brightens, never darkens),
    /// `Multiply`, `Screen` or `PremultipliedAlpha`. Honoured on sprites and
    /// meshes, lit or `Unlit`, by `ecs::wgpu_systems::sprite_blend`; a
    /// material named `"additive"`, `"multiply"` or `"screen"` means the same
    /// without the override. A shader-graph material blends as its
    /// `.material` says. Until Sep 2026 no renderer read this field, so it
    /// drew alpha-blended everywhere. `Screen` assumes colours at most 1.0 -
    /// in an HDR scene a brighter source darkens what is under it; use
    /// `Additive` there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_mode: Option<BlendMode>,
    /// Per-entity uniform overrides — merged on top of the material's base uniforms.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub uniforms: HashMap<String, UniformValue>,
    /// Optional normal map texture for per-pixel lighting (unit 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_map_id: Option<String>,
    /// How strongly point and spot lights glint off the surface (Oct 2026):
    /// 0 is matt (the default), 1 a highlight as bright as the light. With a
    /// normal map the highlights follow its bumps, and the normal map's
    /// **alpha** masks them - paint it dark where the sprite is cloth and
    /// light where it is metal.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub specular: f32,
    /// How tight the highlight is: 1 a broad sheen, 128 a pin-point glint.
    #[serde(default = "default_shininess", skip_serializing_if = "is_default_shininess")]
    pub shininess: f32,
}

fn default_shininess() -> f32 {
    24.0
}
fn is_default_shininess(v: &f32) -> bool {
    *v == default_shininess()
}

impl Default for MaterialComponent {
    fn default() -> Self {
        Self {
            material_id: "default_sprite".into(),
            blend_mode: None,
            uniforms: HashMap::new(),
            normal_map_id: None,
            specular: 0.0,
            shininess: default_shininess(),
        }
    }
}

// ── 2D Lighting ──

/// A point light that illuminates a radial area around the entity's position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointLight2D {
    /// Light color (RGB).
    pub color: [f32; 3],
    /// Light brightness multiplier.
    pub intensity: f32,
    /// Maximum reach in world units.
    pub radius: f32,
    /// Falloff exponent (1.0 = linear, 2.0 = quadratic).
    pub falloff: f32,
    /// Apparent emitter radius in world units used for soft shadow
    /// penumbra. `0.0` = a point emitter ⇒ hard shadows; larger values
    /// widen the penumbra where occluders cast shadows. Ignored when no
    /// `LightOccluder` is present.
    #[serde(default)]
    pub shadow_softness: f32,
    /// How high the light sits above the plane, for normal maps only: the
    /// tangent of its elevation, so `0.5` (the default, and the only value
    /// before Sep 2026) lights a surface from about 27 degrees up. Lower is
    /// more grazing - a normal-mapped sphere shows a night side and a
    /// terminator instead of being lit all over, and relief stands out
    /// harder; higher lights from overhead. A flat surface takes the same
    /// light at any height (the response is relative to flat, so a scene
    /// without normal maps is unchanged), and a face turned to the light
    /// takes at most [`MAX_NORMAL_RESPONSE`] times as much. Clamped to at
    /// least `0.02`.
    #[serde(default = "default_light_height")]
    pub height: f32,
    /// Whether occluders shadow this light. On by default (every light was
    /// shadowed before Sep 2026, and scenes were built that way); turn it off
    /// for a glow, a fill or a light that sits *on* the thing that would
    /// shadow it - a crystal atop a post, whose own post otherwise throws a
    /// wedge of dark down the floor below it. Godot's `shadow_enabled`, and
    /// Unity's shadow strength at zero.
    #[serde(default = "default_casts_shadows")]
    pub casts_shadows: bool,
    /// A texture the light shines through (Oct 2026) - a **cookie**, as
    /// Unity and Godot call it: a window's grille, leaves, a stained-glass
    /// pattern. Its picture spans the light's reach (`radius` times
    /// `cookie_scale` each way from the light), turns with the entity, and
    /// multiplies what the light gives: white passes, black stops, a colour
    /// tints, and alpha fades it. Outside the picture the light gives
    /// nothing, so a cookie is also the light's shape. Empty is none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookie: String,
    /// How far the cookie's picture reaches, as a share of `radius` from the
    /// light to each edge (1, the default, is the light's whole reach).
    #[serde(default = "default_cookie_scale")]
    pub cookie_scale: f32,
    /// The entity's turn, which turns the cookie - filled from its world
    /// transform by `gather_lights`, never authored or saved.
    #[serde(skip)]
    pub turn: f32,
    /// The light layers it lights (Oct 2026; Godot's `range_item_cull_mask`,
    /// Unity's target sorting layers): a sprite is lit by it when the
    /// sprite's `light_layers` shares a bit with this. Every layer by
    /// default. Eight layers, the low eight bits.
    #[serde(default = "all_light_layers", skip_serializing_if = "is_all_light_layers")]
    pub light_mask: u32,
    /// Takes light away instead of adding it (Oct 2026; Godot's Light2D
    /// `BLEND_MODE_SUB`): a cursed lantern that casts darkness, a shadow
    /// cast by nothing. Drawn after the lights that add, and never below
    /// black. Its specular highlight subtracts too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subtract: bool,
    /// How much its pool is squashed top to bottom (Oct 2026): 1 a circle,
    /// less an ellipse as wide as `radius` and `radius * squash` tall - a
    /// lamp's pool on a floor seen in perspective (`ecs::stage` sets it from
    /// the pitch). Shadows and normal maps are unchanged.
    #[serde(default = "default_squash", skip_serializing_if = "is_unsquashed")]
    pub squash: f32,
}

fn default_casts_shadows() -> bool {
    true
}

fn default_squash() -> f32 {
    1.0
}

fn is_unsquashed(v: &f32) -> bool {
    *v == 1.0
}

/// Every light layer: what a light lights unless its `light_mask` says less.
pub const ALL_LIGHT_LAYERS: u32 = u32::MAX;

fn all_light_layers() -> u32 {
    ALL_LIGHT_LAYERS
}

fn is_all_light_layers(v: &u32) -> bool {
    *v == ALL_LIGHT_LAYERS
}

fn default_cookie_scale() -> f32 {
    1.0
}

/// The default [`PointLight2D::height`] / [`SpotLight2D::height`]: the
/// elevation every light had before it could be chosen.
pub const DEFAULT_LIGHT_HEIGHT: f32 = 0.5;

/// The most a normal-mapped face turned to a light takes, relative to a flat
/// surface. Without a cap a grazing light would make the lit limb of a sphere
/// arbitrarily bright (the response is `N.L / L.z`).
pub const MAX_NORMAL_RESPONSE: f32 = 2.5;

fn default_light_height() -> f32 {
    DEFAULT_LIGHT_HEIGHT
}

impl Default for PointLight2D {
    fn default() -> Self {
        Self {
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            radius: 200.0,
            falloff: 2.0,
            shadow_softness: 0.0,
            height: DEFAULT_LIGHT_HEIGHT,
            casts_shadows: true,
            cookie: String::new(),
            cookie_scale: 1.0,
            turn: 0.0,
            light_mask: ALL_LIGHT_LAYERS,
            subtract: false,
            squash: 1.0,
        }
    }
}

/// A light from far away - the sun, the moon: every ray parallel, the same
/// strength everywhere, and every shadow thrown the same way. Godot's
/// `DirectionalLight2D`.
///
/// Before Sep 2026 a sun was a point light placed thousands of units off
/// screen (the showcase's Clay), whose shadows still fanned out a little
/// across the view, and whose shadows could not lengthen as it set. This is
/// that, done by the engine: `renderer::lighting::sun_as_point` puts the
/// light far out along its direction, beyond the view, so every consumer of
/// point lights - the GPU pass, the occluder packing, `light_at` - handles it
/// unchanged.
///
/// A sun is swung by turning its entity (as a spot light is aimed), and set
/// by lowering `height`: shadows lengthen as it does.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectionalLight2D {
    /// Light color (RGB).
    pub color: [f32; 3],
    /// Light brightness multiplier.
    pub intensity: f32,
    /// Which way the light travels, in radians (`0` is towards +x, `-PI/2`
    /// straight down the screen, the default). The entity's rotation is
    /// added.
    #[serde(default = "default_sun_direction")]
    pub direction: f32,
    /// How high the sun stands: the tangent of its elevation, `1` at 45
    /// degrees. It lights normal maps as a point light's `height` does, and it
    /// sets how long shadows are - an occluder's `shadow_length` is the shadow
    /// it throws under a sun at height `1`, and a sun at `h` throws it
    /// `shadow_length / h` long. Occluders with no `shadow_length` cast shadows
    /// to the edge of the view at any height. Clamped to at least `0.02`.
    #[serde(default = "default_sun_height")]
    pub height: f32,
    /// The sun's apparent size, in radians: how soft the edges of its shadows
    /// are, widening with distance from the occluder. `0` is hard.
    #[serde(default)]
    pub softness: f32,
    /// Whether occluders shadow this light.
    #[serde(default = "default_casts_shadows")]
    pub casts_shadows: bool,
    /// The light layers it lights (Oct 2026; Godot's `range_item_cull_mask`,
    /// Unity's target sorting layers): a sprite is lit by the sun when the
    /// sprite's `light_layers` shares a bit with this. Every layer by
    /// default. Eight layers, the low eight bits.
    #[serde(default = "all_light_layers", skip_serializing_if = "is_all_light_layers")]
    pub light_mask: u32,
}

fn default_sun_height() -> f32 {
    1.0
}

fn default_sun_direction() -> f32 {
    -std::f32::consts::FRAC_PI_2
}

impl Default for DirectionalLight2D {
    fn default() -> Self {
        Self {
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            direction: default_sun_direction(),
            height: default_sun_height(),
            softness: 0.0,
            casts_shadows: true,
            light_mask: ALL_LIGHT_LAYERS,
        }
    }
}

/// A directional cone light attached to an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotLight2D {
    /// Light color (RGB).
    pub color: [f32; 3],
    /// Light brightness multiplier.
    pub intensity: f32,
    /// Maximum reach in world units.
    pub radius: f32,
    /// Cone half-angle in radians.
    pub angle: f32,
    /// Direction angle in radians (0 = right, π/2 = up).
    pub direction: f32,
    /// Falloff exponent.
    pub falloff: f32,
    /// Apparent emitter radius in world units used for soft shadow
    /// penumbra (see [`PointLight2D::shadow_softness`]).
    #[serde(default)]
    pub shadow_softness: f32,
    /// How high the light sits above the plane, for normal maps - see
    /// [`PointLight2D::height`].
    #[serde(default = "default_light_height")]
    pub height: f32,
    /// Whether occluders shadow this light - see
    /// [`PointLight2D::casts_shadows`].
    #[serde(default = "default_casts_shadows")]
    pub casts_shadows: bool,
    /// A texture the light shines through (Oct 2026) - a **cookie**, as
    /// Unity and Godot call it: a window's grille, leaves, a stained-glass
    /// pattern. Its picture spans the light's reach (`radius` times
    /// `cookie_scale` each way from the light), turns with the spot's aim, and
    /// multiplies what the light gives: white passes, black stops, a colour
    /// tints, and alpha fades it. Outside the picture the light gives
    /// nothing, so a cookie is also the light's shape. Empty is none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookie: String,
    /// How far the cookie's picture reaches, as a share of `radius` from the
    /// light to each edge (1, the default, is the light's whole reach).
    #[serde(default = "default_cookie_scale")]
    pub cookie_scale: f32,
    /// The light layers it lights (Oct 2026; Godot's `range_item_cull_mask`,
    /// Unity's target sorting layers): a sprite is lit by it when the
    /// sprite's `light_layers` shares a bit with this. Every layer by
    /// default. Eight layers, the low eight bits.
    #[serde(default = "all_light_layers", skip_serializing_if = "is_all_light_layers")]
    pub light_mask: u32,
    /// Takes light away instead of adding it (Oct 2026; Godot's Light2D
    /// `BLEND_MODE_SUB`): a cursed lantern that casts darkness, a shadow
    /// cast by nothing. Drawn after the lights that add, and never below
    /// black. Its specular highlight subtracts too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subtract: bool,
    /// How much its pool is squashed top to bottom (Oct 2026): 1 a circle,
    /// less an ellipse as wide as `radius` and `radius * squash` tall - a
    /// lamp's pool on a floor seen in perspective (`ecs::stage` sets it from
    /// the pitch). Shadows and normal maps are unchanged.
    #[serde(default = "default_squash", skip_serializing_if = "is_unsquashed")]
    pub squash: f32,
}

impl Default for SpotLight2D {
    fn default() -> Self {
        Self {
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            radius: 300.0,
            angle: std::f32::consts::FRAC_PI_4, // 45 degrees
            direction: 0.0,
            falloff: 2.0,
            shadow_softness: 0.0,
            height: DEFAULT_LIGHT_HEIGHT,
            casts_shadows: true,
            cookie: String::new(),
            cookie_scale: 1.0,
            light_mask: ALL_LIGHT_LAYERS,
            subtract: false,
            squash: 1.0,
        }
    }
}

/// Scene-wide ambient light. Attach to any entity (typically the camera or a
/// dedicated "Lighting" entity). Only the first one found is used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbientLight {
    /// Ambient color (RGB).
    pub color: [f32; 3],
    /// Ambient intensity (0 = total darkness, 1 = fully lit).
    pub intensity: f32,
}

impl Default for AmbientLight {
    fn default() -> Self {
        Self {
            color: [1.0, 1.0, 1.0],
            intensity: 0.3,
        }
    }
}

/// **Ambient occlusion**: the ambient light darkened near the outlines of
/// `LightOccluder`s, as it is where two surfaces meet - the soft darkening
/// at the foot of a wall, in a corner, between two boxes set close. Put it
/// on any one entity (the first found is used), beside the `ambient_light`.
///
/// A point `d` from an occluder's outline keeps `1 - strength * (1 -
/// d/radius)^2` of the ambient light from that occluder, and nothing within
/// `radius` of none; near two occluders the two multiply, so a corner or a
/// narrow gap goes darker than one wall. A point *inside* an occluder takes
/// none from that occluder - the top of a box is not darkened by its own
/// sides. Only the ambient term is occluded: a lamp still lights a corner.
/// Godot builds the same thing from its occluders' distance field; the
/// showcase's Clay scene is built on it. `renderer::lighting::ambient_occlusion_at`
/// is the rule, on the CPU and (transcribed) on the GPU.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AmbientOcclusion {
    /// How far from an outline the darkening reaches, in world units.
    #[serde(default = "default_ao_radius")]
    pub radius: f32,
    /// How dark it gets at the outline itself: 0 none, 1 black.
    #[serde(default = "default_ao_strength")]
    pub strength: f32,
}

fn default_ao_radius() -> f32 {
    48.0
}

fn default_ao_strength() -> f32 {
    0.6
}

impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self { radius: default_ao_radius(), strength: default_ao_strength() }
    }
}

/// **Global illumination** (Oct 2026): light that bounces. The lit outlines
/// of the occluders - the scene's colour times the direct light on them -
/// shine again, and every pixel gathers what reaches it round the occluders
/// between, tinted by what it came off: sun through red glass onto a stone
/// floor warms the walls beside it. Radiance cascades, on the GPU
/// (`renderer::backend::wgpu::gi`), added to the light buffer before the
/// composite. Godot and Unity have no 2D equivalent built in. One per scene,
/// like ambient occlusion; `Lighting.set_global_illumination` sets it.
/// `light_at` leaves bounced light out.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GlobalIllumination {
    /// How much of the bounce is added: 1 is what reaches each pixel.
    pub strength: f32,
    /// Frame pixels to a GI texel each way: the bounce is soft, so it is
    /// worked out small. 4 by default; 2 is finer and four times the work.
    pub scale: u32,
    /// How many cascades - each reaches four times as far as the last. 0,
    /// the default, takes as many as cross the frame.
    pub cascades: u32,
}

impl Default for GlobalIllumination {
    fn default() -> Self {
        Self { strength: 1.0, scale: 4, cascades: 0 }
    }
}

/// **Toon lighting**: the light that reaches each pixel is stepped into
/// `bands` levels instead of falling off smoothly - cel shading. A normal-mapped
/// sphere under one light shows a lit face, a mid band and a shadow band with
/// crisp edges between them, as an anime or comic render does. Put it on any
/// one entity (the first found is used).
///
/// Applied in the lighting composite to the light buffer - ambient, lights and
/// lightmaps summed - by its brightest channel, so a light's colour is kept.
/// `light_at` applies the same rule (`renderer::lighting::toon_step`).
///
/// Each point goes to its **nearest** band, so light under half a band - an
/// ambient below `1 / (2 * bands)` - is black: hard cel shadows. For a lit
/// shadow band, keep the ambient above that (0.17 for three bands).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ToonLighting {
    /// How many steps light takes between dark and full (at 1.0). Above 1.0
    /// in an HDR scene the steps continue.
    pub bands: u32,
    /// How soft the edge between two bands is, as a fraction of a band: `0`
    /// is a hard line, `1` is smooth again.
    pub softness: f32,
    /// Dithered lighting: the size of a dither cell in **world units**, `0`
    /// for none. The `softness` of each band's edge is then drawn as an
    /// ordered (4x4 Bayer) pattern of the two bands instead of a gradient -
    /// the look of a palette-limited retro game, whose light could only
    /// come in a few shades. Anchored to the world, so a cell the size of an
    /// art pixel stays on that pixel as the camera moves (and lands on the
    /// `pixelate` grid when both use it).
    pub dither: f32,
}

impl Default for ToonLighting {
    fn default() -> Self {
        Self { bands: 3, softness: 0.08, dither: 0.0 }
    }
}

/// Baked lightmap: pre-computed light, added into the light buffer as a
/// textured quad centred on the entity, before the scene is multiplied by
/// it. A bright texel lights the scene under it exactly as a light would;
/// black adds nothing. The quad is `size` world units (times the transform's
/// scale), rotated with the entity.
///
/// Drawn by `ecs::wgpu_systems::render_lightmaps_wgpu` in every runtime's
/// lighting pass. Until Sep 2026 nothing drew it at all - this comment
/// described a renderer that did not exist (test-lab BUGS #95).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightmapComponent {
    /// Asset path to the baked lightmap texture.
    pub texture_id: String,
    /// UV sub-region within the lightmap texture, `[u_min, v_min, u_max, v_max]`.
    #[serde(default = "default_uv_rect")]
    pub uv_rect: [f32; 4],
    /// Intensity multiplier applied to the texture's colour.
    #[serde(default = "default_lightmap_intensity")]
    pub intensity: f32,
    /// World size of the quad, before the transform's scale.
    #[serde(default = "default_lightmap_size")]
    pub size: [f32; 2],
}

fn default_lightmap_size() -> [f32; 2] {
    [256.0, 256.0]
}

fn default_uv_rect() -> [f32; 4] {
    [0.0, 0.0, 1.0, 1.0]
}
fn default_lightmap_intensity() -> f32 {
    1.0
}

impl Default for LightmapComponent {
    fn default() -> Self {
        Self {
            texture_id: Default::default(),
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            intensity: 1.0,
            size: default_lightmap_size(),
        }
    }
}

fn default_true_occ() -> bool {
    true
}

fn is_true_occ(v: &bool) -> bool {
    *v
}

/// Marks an entity as a light blocker. During the lighting pass each
/// occluder contributes world-space edge segments that point and spot
/// lights ray-test against — fragments whose line-of-sight to a light
/// is crossed by any segment receive no (or, with soft shadows,
/// reduced) contribution from that light, producing cast shadows /
/// field-of-view wedges.
///
/// Shape resolution priority (highest first):
/// 1. `polygon` — an explicit local-space polygon (CCW, closed
///    automatically). Each consecutive pair of points is one edge.
/// 2. `from_alpha` — the silhouette of the sprite's opaque pixels, traced
///    from its texture (`ecs::alpha_shape`); its box until the pixels load.
/// 3. `from_sprite` — derive an axis-aligned box from the entity's
///    `SpriteComponent` size (or its explicit `size` override).
/// 4. `size` — a plain box `size` world units wide/tall.
///
/// All shapes are transformed by the entity's `Transform`
/// (position + rotation + scale).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightOccluder {
    /// When false the occluder is ignored (casts no shadow).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Full width/height of the blocking box in world units, before
    /// the entity's `Transform.scale` is applied. Used when neither
    /// `polygon` nor `from_sprite` apply.
    #[serde(default = "default_occluder_size")]
    pub size: [f32; 2],
    /// Derive the blocking box from the entity's `SpriteComponent`
    /// bounds instead of `size`.
    #[serde(default)]
    pub from_sprite: bool,
    /// Explicit local-space polygon outline (CCW). Overrides `size` /
    /// `from_sprite` when non-empty. Needs at least 3 points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polygon: Vec<[f32; 2]>,
    /// RGB colour the shadow lets through when `shadow_opacity` is high.
    /// `[0, 0, 0]` (the default) produces a neutral shadow; a coloured
    /// tint like `[1, 0, 0]` makes a translucent occluder pass only the
    /// red channel, simulating coloured glass.
    #[serde(default = "default_shadow_tint")]
    pub shadow_tint: [f32; 3],
    /// How opaque the occluder is in `[0, 1]`. `1.0` (default) fully
    /// blocks light (tinted by `shadow_tint`); `0.0` casts no shadow at
    /// all; values in between produce translucent shadows. The light
    /// passing through equals `mix([1,1,1], shadow_tint, shadow_opacity)`.
    #[serde(default = "default_shadow_opacity")]
    pub shadow_opacity: f32,
    /// Whether the occluder shadows **its own inside** - what Unity calls
    /// `ShadowCaster2D.selfShadows`. Off (the default since Sep 2026), a
    /// point inside the outline takes light as if the occluder were not
    /// there, while everything beyond it is shadowed as before - so a lit
    /// wall, pillar or crate that casts a shadow is lit itself, and its
    /// normal map shades it.
    ///
    /// Until then every occluder shadowed its inside: the ray from a light
    /// to a point inside crosses the outline on its way out, so a lit sprite
    /// under its own occluder drew black, and the advice was to draw it
    /// `unlit`, or - the showcase's Crypt - to give a column an occluder of
    /// zero width. Turn it on for an occluder that stands for something
    /// solid whose inside should be dark, like a wall seen from above whose
    /// top the light cannot reach.
    ///
    /// A light *inside* the outline is still boxed in by it: points outside
    /// are shadowed, points inside are lit.
    #[serde(default)]
    pub self_shadow: bool,
    /// Whether its shadow falls on other **casters** - sprites whose entity
    /// has an occluder too. On (the default) is top-down lighting, where a
    /// wall shadows the crate beside it. Off is a side view (Sep 2026): a
    /// tower's blocks stand in front of the wall behind them, so each casts
    /// onto the wall and not onto the blocks beside it - with it on, every
    /// stone was in its neighbours' shadow and the tower drew black. The
    /// GPU knows a caster's pixels from the normal pass; `light_at` has no
    /// pixels and treats every point as not a caster.
    #[serde(default = "default_true_occ", skip_serializing_if = "is_true_occ")]
    pub shadows_casters: bool,
    /// How far the shadow reaches past the occluder, in world units; `0`
    /// (the default) is unbounded. The last quarter fades out.
    ///
    /// Lighting here is flat land: every occluder is infinitely tall, so a
    /// shadow runs on to the edge of the light. That is right for a wall and
    /// wrong for a bench under a far sun, whose shadow in a top-down view is
    /// a short patch - unbounded, the first pew in a nave shadows every pew
    /// behind it (the showcase's Stained Glass met exactly that). Set it to
    /// the length the shadow would have: roughly the object's height over
    /// the tangent of the light's elevation. Up to about 8,000 units, in
    /// steps of four.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub shadow_length: f32,
    /// Cast the **shape of the sprite** (Oct 2026): the outline of its
    /// opaque pixels, traced from the texture - a tree's crown and trunk,
    /// a fence's pickets - rather than its box. Each island of the sprite
    /// is a loop; holes are left out.
    #[serde(default, skip_serializing_if = "is_false_occ")]
    pub from_alpha: bool,
    /// The alpha above which a pixel is part of the shape (0..1).
    #[serde(default = "default_alpha_cutoff")]
    pub alpha_cutoff: f32,
    /// How far, in texels, the traced outline may stray from the pixels'
    /// edge - larger is fewer corners. Every corner is an edge each nearby
    /// light spends from its pool of 448, so a loop is also held to 48.
    #[serde(default = "default_alpha_tolerance")]
    pub alpha_tolerance: f32,
    /// The traced outline, kept until the sprite's texture, region, size
    /// or anchor change (`ecs::alpha_shape::trace_occluder_shapes`). Never
    /// saved.
    #[serde(skip)]
    pub traced: Option<crate::ecs::alpha_shape::TracedShape>,
}

/// A **moving surface** (Oct 2026; Unity's SurfaceEffector2D): whatever
/// rests on this entity's collider is carried along it at `speed` world
/// units a second, along the entity's own +x (turn the entity to aim it;
/// negative runs the other way) - a conveyor belt, a moving walkway, a
/// river's surface, a treadmill. The collider itself does not move: rapier
/// is told the contact's surface slides (`SolverContact::tangent_velocity`),
/// so friction carries what touches it, the way a real belt does, and a
/// body on a slope of belt still slides if friction cannot hold it.
/// `Physics.set_conveyor_speed` changes the speed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Conveyor {
    pub speed: f32,
}

/// A collider in the **shape of its sprite** (Oct 2026; Unity's
/// PolygonCollider2D made from a sprite, Godot's "create collision polygon"):
/// the outline of the sprite's opaque pixels - traced as
/// `light_occluder.from_alpha` traces a shadow (`ecs::alpha_shape`) and
/// scaled by the entity's transform - is written into the entity's own
/// `collider` as a [`ColliderShape::ConvexDecomposition`], so physics, rays,
/// shape queries, particle collisions, navigation and the F4 outlines all
/// read one shape and none of them was touched. A `collider` is added if
/// the entity has none, so friction and the rest can still be authored on
/// one. Every island of the sprite is a loop of the shape (holes are left
/// out).
/// A body waits for its shape - nothing is simulated with a placeholder -
/// and is given the new one when the sprite's texture, region, size, anchor
/// or scale change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SpriteCollider {
    /// The alpha above which a pixel is solid (0..1).
    pub alpha_cutoff: f32,
    /// How far, in texels, the outline may stray from the pixels' edge -
    /// larger is fewer, larger convex pieces.
    pub alpha_tolerance: f32,
    /// What the written shape was traced from (texture, frame, cutoff,
    /// tolerance, scale); 0 until it is. Never saved.
    #[serde(skip)]
    pub key: u64,
}

impl Default for SpriteCollider {
    fn default() -> Self {
        Self { alpha_cutoff: 0.5, alpha_tolerance: 1.5, key: 0 }
    }
}

fn is_false_occ(v: &bool) -> bool {
    !*v
}

fn default_alpha_cutoff() -> f32 {
    0.5
}

fn default_alpha_tolerance() -> f32 {
    1.5
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

fn default_occluder_size() -> [f32; 2] {
    [32.0, 32.0]
}

fn default_true() -> bool {
    true
}

fn default_shadow_tint() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}

fn default_shadow_opacity() -> f32 {
    1.0
}

impl Default for LightOccluder {
    fn default() -> Self {
        Self {
            enabled: true,
            size: [32.0, 32.0],
            from_sprite: false,
            polygon: Vec::new(),
            shadow_tint: [0.0, 0.0, 0.0],
            shadow_opacity: 1.0,
            self_shadow: false,
            shadow_length: 0.0,
            shadows_casters: true,
            from_alpha: false,
            alpha_cutoff: 0.5,
            alpha_tolerance: 1.5,
            traced: None,
        }
    }
}

/// 2D velocity for simple kinematic motion (no physics engine needed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Velocity2D {
    /// Horizontal velocity in pixels/sec.
    pub x: f32,
    /// Vertical velocity in pixels/sec.
    pub y: f32,
    /// Gravity acceleration in pixels/sec² (subtracted from y each frame).
    #[serde(default)]
    pub gravity: f32,
}

impl Default for Velocity2D {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, gravity: 0.0 }
    }
}

/// Which scripting language a [`BatchScriptComponent`] is written in.
///
/// Scenes default to `Js` so existing prefabs from Slice 4.1/4.2 (which
/// were JS-only) continue to deserialize without a `lang` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ScriptLang {
    #[default]
    Js,
    Lua,
}


/// Step 4 walking-skeleton: declares that this entity is processed by a
/// **batch script system**. The system runs once per frame for *all*
/// entities carrying a `BatchScriptComponent` whose `(lang, path)` matches,
/// instead of invoking `update(entity, dt)` per entity.
///
/// **Walking-skeleton scope (Step 4.1/4.2)**: the batch's read/write columns
/// are fixed to `Velocity2D` (read x/y) → `Transform.position` (write x/y).
/// `system.query.read/write` declared in the script header is parsed and
/// validated against this fixed contract at load time (Slice 4.2). Slice
/// 4B.1 adds a `lang` field so scripts can be Lua *or* JS — dispatch picks
/// the matching runtime, but everything else (path, schema, mirror buffer
/// layout) is language-agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchScriptComponent {
    /// Path to the script module declaring an `update(ctx, q)` function.
    /// Relative to the project's assets directory, same as
    /// [`ScriptComponent::source_path`].
    pub path: String,
    /// Scripting language. Defaults to `Js` for back-compat with prefabs
    /// authored before Slice 4B.1.
    #[serde(default)]
    pub lang: ScriptLang,
}

impl Default for BatchScriptComponent {
    fn default() -> Self {
        Self { path: String::new(), lang: ScriptLang::Js }
    }
}

/// Axis-aligned bounding box that constrains entity position and bounces velocity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BounceArea {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    /// Energy preservation on bounce (0.0–1.0). 1.0 = perfectly elastic.
    #[serde(default = "default_bounce_damping")]
    pub damping: f32,
}

fn default_bounce_damping() -> f32 {
    0.9
}

impl Default for BounceArea {
    fn default() -> Self {
        Self {
            min_x: -640.0,
            min_y: -360.0,
            max_x: 640.0,
            max_y: 360.0,
            damping: 0.9,
        }
    }
}

// ── Step 2A native-motion primitives ──
//
// These components are intentionally orthogonal to one another so they can be
// composed freely on entities and parallelised by chunk in Step 2C. Do NOT add
// fields that couple two behaviours (e.g. don't bake a velocity field into a
// `KinematicMover` wrapper). The legacy `Velocity2D.gravity` field is kept for
// scene back-compat; new entities should attach a `Gravity2D` component instead.

/// 2D acceleration vector applied to `Velocity2D` each frame
/// (`vel += g * dt`). Pixels/sec². For Earth-like downward gravity, use a
/// negative `y` (the engine's +y is up).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Gravity2D {
    pub x: f32,
    pub y: f32,
}

/// What to do when a `Lifetime`'s `remaining` reaches zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExpireAction {
    /// Despawn the entity entirely.
    #[default]
    Despawn,
}

/// Despawn this entity once it has left the view.
///
/// Every scrolling game needs this and, until Sep 2026, every one of them
/// wrote it again in Lua: walk your own list of spawned things, compare each
/// one against the camera, despawn the ones behind. `Lifetime` covers "this
/// goes away after a while"; this covers "this goes away once nobody can see
/// it", which is the other half.
///
/// Not [`DespawnOutOfBounds`], which is a **fixed** world-space box: in a
/// game whose world scrolls, that box would have to move with the view, and
/// there is nothing to move it. Use that one for a play area that stays
/// put, and this one for anything the camera travels past.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct DespawnOffscreen {
    /// How far outside the camera's view the entity may go before it is
    /// despawned, in world units. A margin gives a thing time to finish
    /// leaving before it disappears, and stops a wide sprite vanishing while
    /// half of it is still on screen.
    pub margin: f32,
    /// Only despawn once it is *behind* the camera's travel - what a
    /// scrolling game wants, because whatever is ahead has not had its turn
    /// yet. "Behind" is judged per axis from the camera's movement since the
    /// last frame: outside the view on the side the camera is moving away
    /// from. A camera that is not moving leaves nothing behind, so a still
    /// frame keeps what is waiting ahead of the view, and so does a camera
    /// that jumped further than the view in one frame (a cut, not travel).
    #[serde(default)]
    pub behind_only: bool,
    /// Whether it has been inside the view (with its margin) yet - set by
    /// the despawn pass itself, never authored. The camera can only have
    /// passed something it has reached: a thing spawned off screen ahead has
    /// not had its turn, however the camera moves before it gets there. A
    /// runner restarting at the start of its track jumps the camera back,
    /// and without this the new run's next chunk, spawned just off screen
    /// on that frame, read as "behind" and was deleted.
    #[serde(skip)]
    pub seen: bool,
}

impl Default for DespawnOffscreen {
    fn default() -> Self {
        Self { margin: 160.0, behind_only: false, seen: false }
    }
}

/// A **trail**: a ribbon left behind the entity as it moves - a sword's
/// swing, a comet's tail, a light painted with the mouse. Unity's
/// `TrailRenderer`, Godot's `Line2D` fed from a script.
///
/// Every frame of play the entity's world position is recorded once it has
/// moved `min_distance` from the last point (`ecs::trails::tick_trails`,
/// which every runtime runs right after resolving the hierarchy); a point
/// lives `lifetime` seconds. `Trail.set_emitting` and `Trail.clear` steer it
/// from a script. Long gaps between points - a fast swing - are drawn as
/// curves, not chords. The ribbon runs from the entity's position this frame
/// back through the points, its width and colour going from `width` /
/// `color` at the head to `width_end` / `color_end` at a point's last
/// moment - so with a `color_end` of alpha 0 or a `width_end` of 0 a point
/// fades out rather than popping off the tail. The texture is stretched
/// along the whole ribbon: `u` 0 at the head and 1 at the tail, `v` across.
///
/// Drawn in the sprite pass (sorted by `z_order` with sprites, lit unless
/// `Unlit`, blended by `blend_mode`). A trail on a child is the trail of
/// where the child is - put one on the tip of a sword, not the hilt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Trail {
    /// How long a point lives, in seconds - the length of the tail in time.
    pub lifetime: f32,
    /// Width at the head, in world units.
    pub width: f32,
    /// Width at a point's last moment.
    pub width_end: f32,
    /// Colour (linear RGBA) at the head.
    pub color: [f32; 4],
    /// Colour at a point's last moment.
    pub color_end: [f32; 4],
    /// How far the entity moves before a new point is recorded. Smaller is
    /// smoother and costs more points (at most [`Trail::MAX_POINTS`]).
    pub min_distance: f32,
    /// Texture stretched along the ribbon; empty is plain colour.
    pub texture_id: String,
    pub z_order: i32,
    pub sort_layer: Option<String>,
    /// Off: no new points are recorded and the ones there age out.
    pub emitting: bool,
    pub blend_mode: crate::renderer::material::BlendMode,
    /// The recorded points, oldest first - kept by `tick_trails`, never
    /// authored.
    #[serde(skip)]
    pub points: Vec<TrailPoint>,
}

/// One recorded point of a [`Trail`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailPoint {
    pub position: Vec2,
    /// Seconds since it was recorded.
    pub age: f32,
}

impl Trail {
    /// A trail keeps at most this many points; past it the oldest go first.
    pub const MAX_POINTS: usize = 256;
}

impl Default for Trail {
    fn default() -> Self {
        Self {
            lifetime: 0.5,
            width: 16.0,
            width_end: 0.0,
            color: [1.0, 1.0, 1.0, 1.0],
            color_end: [1.0, 1.0, 1.0, 0.0],
            min_distance: 4.0,
            texture_id: Default::default(),
            z_order: 0,
            sort_layer: None,
            emitting: true,
            blend_mode: crate::renderer::material::BlendMode::Alpha,
            points: Vec::new(),
        }
    }
}

/// On-screen controls for a touch screen: sticks and buttons that press keys.
///
/// A phone has no keyboard, and the games here read one. A control presses a
/// **key** - named directly (`"Space"`) or through an input action (`"jump"`
/// presses the first keyboard key bound to it) - so a keyboard game plays by
/// touch unchanged. Put one on any entity in a scene that wants them; a scene
/// without one (a menu) is tapped instead. The logic, and what each runtime
/// has to do, is in `input::touch_controls`.
///
/// ```json
/// "touch_controls": {
///   "controls": [
///     { "type": "stick", "anchor": "bottom_left", "offset": [160, 160],
///       "up": "Up", "down": "Down", "left": "Left", "right": "Right" },
///     { "type": "button", "anchor": "bottom_right", "offset": [130, 130],
///       "press": "jump", "label": "A" }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchControls {
    /// When the controls show (and take fingers): `touch` - once the screen
    /// has been touched, so a desktop never sees them; `always` - including
    /// on a desktop, where the mouse works them (for trying a layout); or
    /// `never`.
    pub show: TouchControlsShow,
    /// Multiplies every control's opacity, 0..1.
    pub opacity: f32,
    pub controls: Vec<TouchControl>,
}

impl Default for TouchControls {
    fn default() -> Self {
        Self { show: TouchControlsShow::Touch, opacity: 1.0, controls: Vec::new() }
    }
}

/// When [`TouchControls`] show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchControlsShow {
    /// Once a finger has touched the screen.
    #[default]
    Touch,
    Always,
    Never,
}

/// One on-screen control.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TouchControl {
    Stick(TouchStick),
    Button(TouchButton),
}

/// A thumbstick pressing up to two of four directions (a diagonal is two).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchStick {
    pub anchor: ScreenAnchor,
    /// From the anchored edge inward, in UI pixels (see
    /// `input::touch_controls::place`).
    pub offset: [f32; 2],
    pub radius: f32,
    /// How far from the centre, as a fraction of `radius`, before a
    /// direction presses.
    pub dead_zone: f32,
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
}

impl Default for TouchStick {
    fn default() -> Self {
        Self {
            anchor: ScreenAnchor::BottomLeft,
            offset: [160.0, 160.0],
            radius: 100.0,
            dead_zone: 0.3,
            up: "Up".into(),
            down: "Down".into(),
            left: "Left".into(),
            right: "Right".into(),
        }
    }
}

/// A round button holding one key down while pressed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchButton {
    pub anchor: ScreenAnchor,
    pub offset: [f32; 2],
    pub radius: f32,
    /// A key name or an input action.
    pub press: String,
    /// Drawn on the button; empty for none.
    pub label: String,
}

impl Default for TouchButton {
    fn default() -> Self {
        Self {
            anchor: ScreenAnchor::BottomRight,
            offset: [130.0, 130.0],
            radius: 60.0,
            press: String::new(),
            label: String::new(),
        }
    }
}

/// The screen edge or corner a touch control is placed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    #[default]
    BottomLeft,
    Bottom,
    BottomRight,
}

/// Countdown timer that fires `on_expire` once `remaining <= 0`.
///
/// `update_lifetimes` ticks `remaining -= dt` each frame and queues the
/// configured action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Lifetime {
    /// Remaining time in seconds. When this reaches zero, `on_expire` fires.
    pub remaining: f32,
    #[serde(default)]
    pub on_expire: ExpireAction,
}

impl Default for Lifetime {
    fn default() -> Self {
        Self {
            remaining: 1.0,
            on_expire: ExpireAction::Despawn,
        }
    }
}

// ── Step 2D native behavior set ──
//
// Each component is intentionally orthogonal so it can be composed freely
// and parallelised by chunk. A "behavior" here is data + a tiny pure system;
// no scripting, no host crossings. See docs/archive/scripting-redesign-plan.md
// "Step 2D" for the full design.

/// Per-entity acceleration applied to `Velocity2D` each frame
/// (`vel += a * dt`). Distinct from `Gravity2D` in *intent* (gravity is the
/// world's constant downward pull; `Acceleration2D` is per-entity transient
/// acceleration — engines, thrusters, drag forces, AI steering output).
/// Composes additively with `Gravity2D`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Acceleration2D {
    pub x: f32,
    pub y: f32,
}

/// 2D velocity-proportional drag (`vel *= (1 - drag * dt)` per axis,
/// clamped at zero). Integrated by `behaviors::update_forces` in every
/// runtime since Sep 2026 - before that no native system read it, and it
/// was only vocabulary for batch scripts, which still may read it (an
/// entity with a running batch script is left to that script).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Drag2D {
    pub x: f32,
    pub y: f32,
}

/// 2D force on a unit mass: `vel += force * dt`, integrated by
/// `behaviors::update_forces` (Sep 2026; batch-script vocabulary only
/// before that).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Force2D {
    pub x: f32,
    pub y: f32,
}

/// Ambient 2D wind: `vel += wind * dt`, integrated by
/// `behaviors::update_forces` (Sep 2026). Distinct from `Force2D` so an
/// entity can carry both - its own push and the weather's.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Wind2D {
    pub x: f32,
    pub y: f32,
}

/// Instantaneous 2D impulse: added to the velocity once and then cleared,
/// by `behaviors::update_forces` (Sep 2026). Set it again - from a graph's
/// `SetCustomField`, a script's component write - for another kick.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Impulse2D {
    pub x: f32,
    pub y: f32,
}

/// Spin-up: `AngularVelocity.omega += x * dt` each frame, by
/// `behaviors::update_forces` (Sep 2026). `y` is unused - the two fields
/// keep the layout of the rest of the batch vocabulary.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Torque2D {
    pub x: f32,
    pub y: f32,
}

/// Clamps the magnitude of `Velocity2D` to at most `value` (pixels/sec).
/// Run after gravity/acceleration integration, before bounce.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MaxSpeed {
    pub value: f32,
}

impl Default for MaxSpeed {
    fn default() -> Self {
        Self { value: 1000.0 }
    }
}

/// Sinusoidal positional motion. Each frame:
/// `position = center + (sin(2π·freq·t + phase) * amplitude)` per axis.
///
/// On the first tick (when `center` is `None`), the entity's current
/// `Transform.position` is captured as the oscillation center — this lets
/// a designer drop an `Oscillator` on any sprite without having to set the
/// center manually.
///
/// Oscillator is **self-contained** — it sets `Transform.position`
/// directly and does not touch `Velocity2D`. If you want a sprite that
/// drifts AND oscillates, prefer combining a velocity-driven entity with
/// a child Oscillator entity rather than stacking them on the same one.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Oscillator {
    /// Peak displacement per axis, in world units.
    pub amplitude: [f32; 2],
    /// Cycles per second per axis.
    pub frequency: [f32; 2],
    /// Phase offset per axis, in radians.
    #[serde(default)]
    pub phase: [f32; 2],
    /// Accumulated time in seconds.
    #[serde(default)]
    pub elapsed: f32,
    /// Captured oscillation center. `None` means "capture from current
    /// `Transform.position` on first tick".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f32; 2]>,
}

impl Default for Oscillator {
    fn default() -> Self {
        Self {
            amplitude: [32.0, 0.0],
            frequency: [1.0, 0.0],
            phase: [0.0, 0.0],
            elapsed: 0.0,
            center: None,
        }
    }
}

/// A reference to another entity that is stable across save/load.
///
/// Raw `hecs::Entity` is fine at runtime but useless in scene files — the
/// generation/index combo is regenerated each load. `EntityRef` lets a
/// component point at "the player", "the entity named X", or "myself" in
/// a way that survives serialization.
///
/// Variants are added incrementally as the codebase needs them. Today
/// `Runtime`, `Named`, `Player` and `SelfEntity` are wired into the resolver;
/// `SceneUuid` is a placeholder for upcoming editor work and resolves to
/// `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
#[derive(Default)]
pub enum EntityRef {
    /// A live runtime `hecs::Entity`. Set programmatically — do NOT
    /// serialize this variant; it will not survive a scene reload.
    #[serde(skip)]
    Runtime(hecs::Entity),
    /// Resolved by scanning for an entity with the given UUID. Reserved
    /// for upcoming editor work.
    SceneUuid(String),
    /// Resolved by scanning for an entity whose `Name` matches.
    Named(String),
    /// The first entity tagged `"player"` (see
    /// [`crate::ecs::behaviors::PLAYER_TAG`]).
    Player,
    /// Resolves to the entity that owns this component.
    #[default]
    SelfEntity,
}


/// Move toward `target` at up to `max_speed` (pixels/sec), stopping when
/// within `stop_distance`. Writes directly to `Transform.position` — does
/// not touch `Velocity2D`. For physics-style steering, use `SeekTarget`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowTarget {
    pub target: EntityRef,
    /// Constant offset added to the target's position.
    #[serde(default)]
    pub offset: [f32; 2],
    pub max_speed: f32,
    /// Don't move when within this many world units of the target+offset.
    #[serde(default)]
    pub stop_distance: f32,
}

impl Default for FollowTarget {
    fn default() -> Self {
        Self {
            target: EntityRef::SelfEntity,
            offset: [0.0, 0.0],
            max_speed: 200.0,
            stop_distance: 4.0,
        }
    }
}

/// Steer `Velocity2D` toward `target` by accelerating at `accel`
/// (pixels/sec²). Velocity magnitude is clamped to `max_speed`. Requires
/// a `Velocity2D` on the same entity (skipped silently otherwise).
///
/// **Arrival** (Step 6d): when `slowing_radius > 0` and the entity is
/// inside that radius of the target, the per-frame speed cap is scaled
/// linearly from `max_speed` (at the radius) down to `0` (at the
/// target), so the seeker decelerates to a stop instead of orbiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeekTarget {
    pub target: EntityRef,
    pub accel: f32,
    pub max_speed: f32,
    /// Step 6d — arrival behavior. `0.0` (default) disables it and the
    /// component behaves exactly as in Step 2D. Positive values enable
    /// linear deceleration inside the radius.
    #[serde(default)]
    pub slowing_radius: f32,
}

impl Default for SeekTarget {
    fn default() -> Self {
        Self {
            target: EntityRef::SelfEntity,
            accel: 400.0,
            max_speed: 300.0,
            slowing_radius: 0.0,
        }
    }
}

/// What scalar property a `Tween` drives. Typed enum (not a dynamic
/// `TweenValue` blob) so the runtime never has to do a per-tick type
/// check — the system matches once per entity per frame.
///
/// Vector tweens (Position2D, SpriteColor) are intentionally omitted in
/// the first cut; compose two scalar tweens, or add them later when a
/// real use case forces the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TweenTarget {
    PositionX,
    PositionY,
    Rotation,
    ScaleX,
    ScaleY,
    /// Sprite color alpha channel.
    Opacity,
}

/// Easing curve. `apply(t)` maps `t ∈ [0, 1]` → eased `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Ease {
    #[default]
    Linear,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
}

impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::EaseInQuad => t * t,
            Ease::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Ease::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - crate::detmath::DetMath::dpowi(-2.0 * t + 2.0, 2) / 2.0
                }
            }
        }
    }
}

/// What to do once a `Tween` reaches its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TweenComplete {
    /// Stop and clamp at `to`. The component stays on the entity but
    /// `elapsed >= duration` so subsequent ticks are no-ops.
    #[default]
    Stop,
    /// Restart from 0.
    Loop,
    /// Swap `from` ↔ `to` and restart.
    PingPong,
}

/// Interpolate a single scalar property from `from` to `to` over `duration`.
/// Designer-friendly defaults: linear ease, stop at end. See `TweenTarget`
/// for the supported properties.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Tween {
    pub target: TweenTarget,
    pub from: f32,
    pub to: f32,
    pub duration: f32,
    #[serde(default)]
    pub elapsed: f32,
    #[serde(default)]
    pub ease: Ease,
    #[serde(default)]
    pub on_complete: TweenComplete,
}

impl Default for Tween {
    fn default() -> Self {
        Self {
            target: TweenTarget::Opacity,
            from: 0.0,
            to: 1.0,
            duration: 1.0,
            elapsed: 0.0,
            ease: Ease::Linear,
            on_complete: TweenComplete::Stop,
        }
    }
}

/// How a `PatrolPathRef` traverses its waypoints once it hits the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatrolMode {
    /// Go on from the last waypoint to waypoint 0 and round again - it
    /// **drives** back, closing the loop; it does not jump. A route whose
    /// start and end are apart (through traffic) wants its return leg off
    /// screen.
    #[default]
    Loop,
    /// Reverse direction and walk back.
    PingPong,
    /// Stop at the last waypoint.
    Once,
}

/// References a named route and walks the entity along it at `speed`
/// (pixels/sec).
///
/// Routes are declared in the scene's top-level `paths` block and collected
/// into a [`crate::ecs::path::Path2DLibrary`], so a hundred patrol guards share
/// one `Vec<Vec2>` rather than carrying a hundred copies. An unknown
/// `path_id` is skipped silently — an asset typo leaves one guard standing
/// still rather than crashing the level.
///
/// The library was populated by nothing at all until Sep 2026: this component,
/// the library and `update_patrol_paths` all existed with tests, and every
/// runtime passed `None`, so patrol entities moved nowhere (BUGS #7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatrolPathRef {
    /// Asset ID into the `Path2DLibrary`.
    pub path_id: String,
    pub speed: f32,
    /// Index of the waypoint we are currently moving toward.
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub mode: PatrolMode,
    /// `true` = walking 0 → N, `false` = walking N → 0 (only used by
    /// `PingPong`).
    #[serde(default = "default_patrol_forward")]
    pub forward: bool,
}

fn default_patrol_forward() -> bool {
    true
}

impl Default for PatrolPathRef {
    fn default() -> Self {
        Self {
            path_id: String::new(),
            speed: 100.0,
            index: 0,
            mode: PatrolMode::Loop,
            forward: true,
        }
    }
}

// ── Shadow Caster ──

/// Shadow caster component — renders a shadow silhouette behind the entity's sprite.
///
/// Requires `Transform` and `SpriteComponent` on the same entity.
/// The shadow is drawn using the sprite's texture with the caster's color tint,
/// offset by `offset * height` in world units.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowCaster {
    /// Whether this shadow caster is active.
    #[serde(default = "default_shadow_enabled")]
    pub enabled: bool,
    /// Shadow tint color (RGBA). Typically dark and semi-transparent.
    #[serde(default = "default_shadow_color")]
    pub color: [f32; 4],
    /// Shadow offset direction in world units (before height multiplier).
    #[serde(default = "default_shadow_offset")]
    pub offset: [f32; 2],
    /// Fake elevation — multiplies offset to push shadow further.
    #[serde(default = "default_shadow_height")]
    pub height: f32,
    /// Shadow scale relative to sprite size.
    #[serde(default = "default_shadow_scale")]
    pub scale: [f32; 2],
    /// Horizontal shear for perspective-style shadows.
    #[serde(default)]
    pub skew: f32,
}

fn default_shadow_enabled() -> bool {
    true
}

fn default_shadow_color() -> [f32; 4] {
    [0.0, 0.0, 0.0, 0.4]
}

fn default_shadow_offset() -> [f32; 2] {
    [2.0, -2.0]
}

fn default_shadow_height() -> f32 {
    1.0
}

fn default_shadow_scale() -> [f32; 2] {
    [1.0, 1.0]
}

impl Default for ShadowCaster {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [0.0, 0.0, 0.0, 0.4],
            offset: [2.0, -2.0],
            height: 1.0,
            scale: [1.0, 1.0],
            skew: 0.0,
        }
    }
}

// ──────────────────────────── Vector Path ────────────────────────────

/// A single path command — moveto, lineto, bezier curves, or close.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum PathCommand {
    /// Move the pen to a new position without drawing.
    MoveTo { x: f32, y: f32 },
    /// Draw a straight line to the given position.
    LineTo { x: f32, y: f32 },
    /// Quadratic bezier curve with one control point.
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    /// Cubic bezier curve with two control points.
    CubicTo {
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    },
    /// Close the current sub-path back to the last MoveTo.
    Close,
}

/// Vector path rendering component — defines 2D shapes via path commands.
///
/// Paths are tessellated into triangles via `lyon` and drawn with `ShapeBatch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorPath {
    /// Ordered list of path drawing commands.
    pub commands: Vec<PathCommand>,
    /// Fill color (RGBA). If None, the path is not filled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<[f32; 4]>,
    /// Stroke/outline color (RGBA). If None, the path is not stroked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<[f32; 4]>,
    /// Stroke width in world units.
    #[serde(default = "default_stroke_width")]
    pub stroke_width: f32,
    /// Draw order — higher z_order draws on top.
    #[serde(default)]
    pub z_order: i32,
    /// Whether the path is closed (auto-close last sub-path).
    #[serde(default)]
    pub closed: bool,
    /// Optional sort-layer name (see `SortLayers`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
}

fn default_stroke_width() -> f32 {
    1.0
}

impl Default for VectorPath {
    fn default() -> Self {
        Self {
            commands: Vec::new(),
            fill_color: Some([1.0, 1.0, 1.0, 1.0]),
            stroke_color: None,
            stroke_width: 1.0,
            z_order: 0,
            closed: false,
            sort_layer: None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Step 6 — native behavior vocabulary expansion
// ─────────────────────────────────────────────────────────────────────────

// ── 6a — Input → entity-level components ──

/// What an [`InputAxis2D`] drives on its own entity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AxisDrive {
    #[default]
    None,
    Velocity,
    Acceleration,
}

/// Reads four named input actions (left/right/down/up) and produces a 2D
/// axis vector each frame. The system [`update_input_actions`] writes
/// `current` based on the global `InputState`; downstream consumers
/// (scripts, motion components) read `current.x` / `current.y`.
///
/// `current` is `#[serde(skip)]` — it is recomputed every frame and is
/// not part of the serialized scene.
///
/// Empty action names ("") are treated as "no binding" for that axis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputAxis2D {
    /// Action name driving negative X (e.g. `"move_left"`).
    #[serde(default)]
    pub x_neg: String,
    /// Action name driving positive X (e.g. `"move_right"`).
    #[serde(default)]
    pub x_pos: String,
    /// Action name driving negative Y (e.g. `"move_down"`).
    #[serde(default)]
    pub y_neg: String,
    /// Action name driving positive Y (e.g. `"move_up"`).
    #[serde(default)]
    pub y_pos: String,
    /// Scale factor applied to the raw axis vector before assignment.
    /// Defaults to `1.0`. Set to a movement speed (px/sec) and feed
    /// `current` directly into [`Velocity2D`] from a script for a
    /// one-line "WASD move" behavior.
    #[serde(default = "default_axis_scale")]
    pub scale: f32,
    /// When `true` and both axes are non-zero, normalize the (pre-scale)
    /// axis vector to unit length so diagonal movement isn't √2 faster.
    #[serde(default)]
    pub normalize: bool,
    /// Computed each frame by [`update_input_actions`]. Not serialized.
    #[serde(skip)]
    pub current: Vec2,
    /// What this axis moves by itself, with no script: `"velocity"` sets
    /// the entity's `Velocity2D` to `current` every frame, `"acceleration"`
    /// its `Acceleration2D`. `"none"` (the default) leaves `current` for a
    /// script to read, as it always did.
    #[serde(default)]
    pub drive: AxisDrive,
}

fn default_axis_scale() -> f32 {
    1.0
}

impl Default for InputAxis2D {
    fn default() -> Self {
        Self {
            x_neg: String::new(),
            x_pos: String::new(),
            y_neg: String::new(),
            y_pos: String::new(),
            scale: 1.0,
            normalize: false,
            current: Vec2::ZERO,
            drive: AxisDrive::None,
        }
    }
}

/// One-shot impulse triggered when a named action is *just pressed*
/// (edge-triggered, not held). Writes directly into [`Velocity2D`] on
/// the same entity. Each axis is optional — `None` leaves the existing
/// component value alone, `Some(v)` replaces it.
///
/// Canonical use: a one-button flap (`action: "jump"`,
/// `impulse_y: Some(300.0)`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputActionImpulse {
    /// Action name to listen for (e.g. `"jump"`).
    pub action: String,
    /// Replacement X velocity on trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impulse_x: Option<f32>,
    /// Replacement Y velocity on trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impulse_y: Option<f32>,
}

// ── Step 7 — behavior graph component ──

/// Attaches a behavior graph (`.behavior` asset) to an entity.
///
/// The graph runs every frame in [`crate::behavior::system::BehaviorGraphSystem`],
/// scheduled at Phase 2 (between input collection and native motion).
/// See [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md) §3.3.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BehaviorGraphComponent {
    /// Path to a `.behavior` asset, resolved against the project's
    /// assets directory (same convention as
    /// [`ScriptComponent::source_path`]).
    pub graph_path: String,

    /// Per-instance overrides for `nodes[].params`, keyed by node id.
    /// Stored as JSON for scene serialization. Slice 7.2 ignores
    /// these; a later slice (7.4+) will resolve them into a typed
    /// `ResolvedOverrides` table at attach time so the runtime never
    /// touches `serde_json::Value` on the hot path.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub overrides: std::collections::HashMap<String, serde_json::Value>,
}

// ── 6b — Trigger / area component ──

/// Shape of a [`TriggerArea2D`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TriggerShape {
    /// Axis-aligned bounding box centered on the entity's transform.
    Aabb { half_extents: [f32; 2] },
    /// Circle centered on the entity's transform.
    Circle { radius: f32 },
}

impl Default for TriggerShape {
    fn default() -> Self {
        TriggerShape::Aabb { half_extents: [16.0, 16.0] }
    }
}

/// Spatial trigger volume that emits enter/exit/staying events into the
/// sibling [`TriggerOverlaps`] component each frame.
///
/// Pure data — the [`update_trigger_areas`] system performs all overlap
/// detection. Other entities are considered "candidates" if they have a
/// `Transform` and at least one tag from `match_tags` (or `match_tags`
/// is empty, in which case every other entity with a `Transform` is
/// tested).
///
/// The `match_tags` field is a deliberately simple alternative to a
/// full physics layer/mask scheme — it composes with the existing
/// [`Tags`] component without introducing a new vocabulary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriggerArea2D {
    pub shape: TriggerShape,
    /// Only entities carrying at least one of these tags are considered
    /// for overlap. Empty list = match every entity with a `Transform`.
    #[serde(default)]
    pub match_tags: Vec<String>,
}

/// Per-frame overlap state written by [`update_trigger_areas`] onto
/// every entity that carries a [`TriggerArea2D`]. Lists are cleared and
/// repopulated each frame; entity ids are `hecs::Entity::to_bits().get()`
/// values for stable cross-frame identification from scripts.
///
/// `entered` and `exited` are edge-triggered (compared to last frame's
/// `staying` set); `staying` is the full overlap set this frame.
#[derive(Debug, Clone, Default)]
pub struct TriggerOverlaps {
    pub entered: Vec<u64>,
    pub staying: Vec<u64>,
    pub exited: Vec<u64>,
}

// ── Combat: data-driven health + trigger hitboxes ──────────────────────
//
// A lightweight combat layer that works without a physics backend. The
// shape/target-filter for a hitbox comes from a sibling
// [`TriggerArea2D`] (+ [`TriggerOverlaps`]); these components add the
// combat semantics on top. Driven by `apply_hitbox_damage`,
// `despawn_dead`, `tick_invuln`, and `mirror_health_to_custom` in
// `ecs::systems`.

fn default_health_max() -> f32 {
    100.0
}

/// Authoritative combat HP for the native damage systems. Scripts read
/// a mirrored copy via the `Health` custom component (written by
/// `mirror_health_to_custom`) and never write this directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub current: f32,
    #[serde(default = "default_health_max")]
    pub max: f32,
    /// Seconds of invulnerability granted after a hit (i-frames).
    /// `0.0` = hittable every frame a hitbox overlaps.
    #[serde(default)]
    pub invuln_time: f32,
    /// Runtime i-frame countdown; not serialized.
    #[serde(default, skip)]
    pub invuln_timer: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self { current: 100.0, max: 100.0, invuln_time: 0.0, invuln_timer: 0.0 }
    }
}

/// An offensive overlap hitbox. Pairs with a sibling [`TriggerArea2D`]
/// (shape + `match_tags` target filter). Each frame `apply_hitbox_damage`
/// damages every overlapping entity that carries a [`Health`], respecting
/// this hitbox's per-target `retrigger` cooldown and the victim's
/// i-frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hitbox {
    pub damage: f32,
    /// Positional knockback distance applied to the victim, directed away
    /// from this hitbox's entity. `0.0` = none.
    #[serde(default)]
    pub knockback: f32,
    /// When false the hitbox deals no damage (toggle for timed swings).
    #[serde(default = "default_true")]
    pub active: bool,
    /// Seconds before the same victim can be hit again by this hitbox.
    /// `0.0` = every frame (bounded only by the victim's i-frames).
    #[serde(default)]
    pub retrigger: f32,
    /// Despawn this hitbox's entity right after it lands a hit. Used for
    /// projectiles (arrows/bullets) that die on impact. Such a hitbox damages
    /// exactly one victim - the nearest it can hurt - even when it overlaps
    /// several on the frame it lands.
    #[serde(default)]
    pub despawn_on_hit: bool,
    /// Runtime per-victim cooldown timers (victim `to_bits` -> seconds
    /// left). Not serialized.
    #[serde(default, skip)]
    pub cooldowns: HashMap<u64, f32>,
}

impl Default for Hitbox {
    fn default() -> Self {
        Self {
            damage: 0.0,
            knockback: 0.0,
            active: true,
            retrigger: 0.0,
            despawn_on_hit: false,
            cooldowns: HashMap::new(),
        }
    }
}

/// Marker: entities carrying this are despawned by `despawn_dead` once
/// their [`Health::current`] reaches `0`. Entities without it (e.g. the
/// player) persist at 0 HP so gameplay scripts can react to death.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DespawnOnDeath {}

/// Marker: this entity's sprite and text ignore 2D lighting.
///
/// World-space sprites and text are drawn into the scene target and then
/// multiplied by the light buffer, so in a dark scene a label is as dark as
/// the floor it sits on - that is right for a sign painted on a wall, and
/// wrong for a caption explaining the scene. An `Unlit` entity is drawn
/// after the lighting composite instead, at full brightness.
///
/// Unlit things sort with lit ones by `z_order` (since Sep 2026): the world
/// pass draws an unlit sprite as a cutout in the lit scene and as itself in
/// an emission buffer the composite adds after the light, so it is full
/// brightness *and* hidden by a lit sprite in front of it. They used to be
/// drawn after all lit content, above it whatever the z - a lit window on a
/// far tower showed through the building in front. Scene 28 uses it for its
/// captions, scene 111 for windows behind buildings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Unlit {}

fn obstacle_default_radius() -> f32 {
    34.0
}

/// Static circular blocker. `resolve_obstacle_collisions` pushes every mover
/// out to `radius` from this entity's position, giving sliding wall/pillar
/// collision without a physics backend. A mover is anything with a
/// [`Health`], or with a [`Velocity2D`] and no `RigidBody2D` (rapier owns
/// those); a `despawn_on_hit` projectile is not pushed but consumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obstacle {
    /// Keep-out radius: movers are pushed so their center sits at least this
    /// far from the obstacle's center. Author it as the visual radius plus a
    /// margin for the mover's own size.
    #[serde(default = "obstacle_default_radius")]
    pub radius: f32,
}

impl Default for Obstacle {
    fn default() -> Self {
        Self { radius: 34.0 }
    }
}

fn separation_default_radius() -> f32 {
    16.0
}

/// Marks an entity spawned in bulk by a system, rather than authored or
/// individually meaningful.
///
/// Three things skip it, for the same reason — nobody addresses it one at a
/// time:
///
/// 1. **The per-frame script mirror.** The browser-script hosts copy every
///    entity's transform into JS each frame so a script *could* call
///    `get_position(e)`. Marked entities are left out.
/// 2. **The editor's Scene panel.** The panel renders one DOM row per entity
///    with several event handlers, and it does not virtualise — 16,000 boids
///    meant ~16,000 rows and roughly 68 ms a frame of layout, against 24 ms of
///    actual engine work. A list that long is useless to a human anyway.
///
/// 3. **Scene snapshots.** `push_scene` / `pop_scene` and the start of a play
///    session serialize the whole world, and that cost is a hitch mid-game
///    rather than something a save dialog pays. Marked entities are left out
///    by [`crate::scene::snapshot_scene_v2`]: they belong to a spawner that
///    recreates them, so writing 16,000 boids into a snapshot preserves state
///    nobody addressed individually. Measured at 16,000: **40.05 ms -> 0.35
///    ms**. A real *save* still keeps every entity — dropping one there would
///    be data loss.
///
/// Put it on flocks, projectiles, debris — anything a system creates by the
/// thousand. Simulation and rendering are completely unaffected.
///
/// The trade is exact: a marked entity's slot in the script mirror is never
/// filled, so `get_position(e)` reads the zeroed default rather than its real
/// transform; it cannot be selected in the editor; and it does not survive a
/// scene push. If you need to read, move or inspect an entity individually —
/// or to have it come back after a `pop_scene` — do not mark it.
///
/// (Named `NoScriptSync` when first added; renamed once it also governed
/// editor visibility, since the old name described only a third of what it
/// does.)
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct BulkEntity;

/// Soft mutual separation: entities carrying this gently push apart from
/// each other when overlapping (crowd spacing). `apply_separation` only
/// considers other `Separation` entities, so the player / props are
/// unaffected unless they opt in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Separation {
    /// Personal-space radius; a pair pushes apart while closer than the sum
    /// of their radii.
    #[serde(default = "separation_default_radius")]
    pub radius: f32,
}

impl Default for Separation {
    fn default() -> Self {
        Self { radius: 16.0 }
    }
}

fn hb_default_width() -> f32 {
    34.0
}
fn hb_default_height() -> f32 {
    5.0
}
fn hb_default_offset() -> f32 {
    26.0
}

/// A floating health bar drawn above an entity that also carries a
/// [`Health`]. Rendered by `sync_health_bars`, which maintains two
/// runtime-only child sprite entities per owner (a dark background and a
/// colored fill that shrinks with HP). Purely cosmetic: authored on the
/// owner; the bar sprites themselves are transient and never serialized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthBar {
    /// Full-width of the bar in world units.
    #[serde(default = "hb_default_width")]
    pub width: f32,
    /// Bar height in world units.
    #[serde(default = "hb_default_height")]
    pub height: f32,
    /// Vertical offset above the owner's origin.
    #[serde(default = "hb_default_offset")]
    pub y_offset: f32,
    /// Hide the bar while HP is full (typical for enemies).
    #[serde(default)]
    pub hide_when_full: bool,
}

impl Default for HealthBar {
    fn default() -> Self {
        Self { width: 34.0, height: 5.0, y_offset: 26.0, hide_when_full: false }
    }
}

/// Runtime-only back-link from a health-bar sprite to its owner. Managed
/// entirely by `sync_health_bars`; not serialized and not authorable.
#[derive(Debug, Clone, Copy)]
pub struct HealthBarView {
    pub owner: hecs::Entity,
    /// `true` for the colored fill quad, `false` for the dark backing.
    pub is_fill: bool,
}

/// Runtime-only back-link from a floating name-tag text entity to its owner.
/// Managed by `sync_name_tags`; not serialized and not authorable. The label
/// string is read from the owner's `CustomComponents["NameTag"]["text"]`, so
/// scripts author it with the existing `set_custom` API (empty = no tag).
#[derive(Debug, Clone, Copy)]
pub struct NameTagView {
    pub owner: hecs::Entity,
}

// ── Step 6 reopen — broader native behavior vocabulary (no spatial index) ──
//
// Each of these is a small, self-contained component driven by a
// dedicated system in `ecs::systems`. They were deferred from the
// initial Step 6 batch so we could ship 6a–6d first; the boid /
// homing-via-spatial-index ops remain deferred and are tracked in
// `docs/random-ideas.md`.

/// Wraps `Transform.position` around a rectangular world-space box
/// (a wrap-around playfield, as in the classic vector shooters). When the entity crosses any edge
/// the position is teleported to the opposite edge — useful for
/// looping playfields, parallax tiles, and bullet-hell screens.
///
/// Pure: only mutates `Transform`. The wrap is unconditional —
/// designers usually pair this with a `Velocity2D` for motion. The
/// box is inclusive on `min` and exclusive on `max` so a position
/// exactly at `max` snaps to `min`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WrapBounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Default for WrapBounds {
    fn default() -> Self {
        Self { min: [-320.0, -240.0], max: [320.0, 240.0] }
    }
}

/// Clamps `Transform.position` inside a rectangular world-space box.
/// Optionally zeros the corresponding `Velocity2D` axis on contact so
/// a player held against a wall doesn't accumulate undelivered
/// velocity.
///
/// Pure: mutates `Transform` and (when `zero_velocity_on_hit` is
/// true) `Velocity2D`. Run AFTER position integration so this
/// frame's motion is what gets clamped.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ClampBounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
    /// Zero the matching `Velocity2D` axis when the entity is pushed
    /// back inside the box. Has no effect on entities without a
    /// `Velocity2D`. Defaults to `true` — the common "keep player on
    /// screen" use case.
    #[serde(default = "default_clamp_zero_vel")]
    pub zero_velocity_on_hit: bool,
}

fn default_clamp_zero_vel() -> bool {
    true
}

impl Default for ClampBounds {
    fn default() -> Self {
        Self {
            min: [-320.0, -240.0],
            max: [320.0, 240.0],
            zero_velocity_on_hit: true,
        }
    }
}

/// Marks an entity for despawn when its `Transform.position` leaves a
/// rectangular world-space box (with an optional outward `margin`).
/// Returned by `update_despawn_out_of_bounds` as a list of
/// `hecs::Entity` for the caller to destroy — same pattern as
/// [`update_lifetimes`].
///
/// Canonical use: bullets / particles that exit the play area.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DespawnOutOfBounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
    /// Extra outward slack before despawn fires. `0.0` despawns the
    /// instant the entity crosses the box edge; positive values let
    /// it travel a bit further before vanishing (useful for sprites
    /// whose visual extends past their pivot).
    #[serde(default)]
    pub margin: f32,
}

impl Default for DespawnOutOfBounds {
    fn default() -> Self {
        Self { min: [-1024.0, -1024.0], max: [1024.0, 1024.0], margin: 0.0 }
    }
}

/// Rotates `Transform.rotation` to face the entity's current
/// `Velocity2D` (radians, atan2 convention). Skips frames where the
/// velocity magnitude is below `min_speed` so a stationary entity
/// doesn't snap to an arbitrary heading.
///
/// `offset_radians` is added to the computed angle — set to π/2 when
/// the sprite art points "up" instead of "right".
///
/// Pure: mutates only `Transform.rotation`. Run AFTER any system
/// that writes `Velocity2D` (gravity, accel, seek, max_speed, input).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FaceVelocity {
    /// Radians added to the computed `atan2(vy, vx)` heading.
    /// Default `0.0` assumes sprite art faces +X.
    #[serde(default)]
    pub offset_radians: f32,
    /// Skip rotation when `|velocity| < min_speed` (px/sec).
    /// Defaults to `1e-3` — effectively "never zero".
    #[serde(default = "default_face_min_speed")]
    pub min_speed: f32,
    /// How fast it turns to its heading, radians per second; `0` (the
    /// default) turns at once, as it always did. A car rounding a corner
    /// wants 6 - 10.
    #[serde(default, skip_serializing_if = "is_zero_f32_face")]
    pub turn_rate: f32,
}

fn is_zero_f32_face(v: &f32) -> bool {
    *v == 0.0
}

impl FaceVelocity {
    /// `rotation` turned toward `heading` (+ the offset) - at once, or by
    /// at most `turn_rate * dt`, the short way round.
    pub fn turn(&self, rotation: f32, heading: f32, dt: f32) -> f32 {
        let want = heading + self.offset_radians;
        if self.turn_rate <= 0.0 {
            return want;
        }
        let d = (want - rotation + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let step = self.turn_rate * dt;
        rotation + d.clamp(-step, step)
    }
}

fn default_face_min_speed() -> f32 {
    1e-3
}

impl Default for FaceVelocity {
    fn default() -> Self {
        Self { offset_radians: 0.0, min_speed: 1e-3, turn_rate: 0.0 }
    }
}

/// Constant angular velocity (radians/sec) applied to
/// `Transform.rotation`. Independent of `Velocity2D` — use this for
/// pure spinning motion (collectibles, gears, propeller blades).
///
/// Pure: mutates only `Transform.rotation`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct AngularVelocity {
    /// Spin rate in radians per second. Positive = counter-clockwise
    /// in the engine's Y-up convention.
    pub omega: f32,
}

/// Periodically toggles `SpriteComponent.color` alpha between `0.0`
/// and the entity's pre-flicker alpha at `frequency` toggles per
/// second, for a fixed `duration`. Auto-removes itself when
/// `elapsed >= duration` and restores the original alpha.
///
/// Canonical use: post-hit "iframe" damage flash on a player or
/// enemy. Requires a `SpriteComponent` on the same entity (skipped
/// silently otherwise).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Flicker {
    /// Toggles per second (e.g. `10.0` = 5 on, 5 off per second).
    pub frequency: f32,
    /// Total flicker time in seconds. Component is removed after this.
    pub duration: f32,
    /// Time elapsed inside this flicker. Initialise to `0.0`.
    #[serde(default)]
    pub elapsed: f32,
    /// Captured alpha at flicker start. `None` means "capture from
    /// `SpriteComponent.color[3]` on first tick" — same self-priming
    /// pattern as `Oscillator.center`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_alpha: Option<f32>,
}

impl Default for Flicker {
    fn default() -> Self {
        Self { frequency: 10.0, duration: 0.5, elapsed: 0.0, original_alpha: None }
    }
}

/// Transient additive jitter offset applied to `Transform.position`
/// for `duration` seconds. Intensity decays linearly from `intensity`
/// at start to `0` at end. Auto-removes itself when expired and
/// restores the entity to its pre-shake position.
///
/// Canonical use: hit-reaction camera shake or impact-frame entity
/// shake. The shake is sampled at `frequency` Hz with linear
/// interpolation between samples so the motion stays jittery rather
/// than a smooth sine.
///
/// Pure: mutates only `Transform.position`, and only by the jitter: each
/// tick removes last tick's offset and adds a new one, so whatever else moves
/// the entity keeps moving it. When the shake ends the offset is removed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Shake {
    /// Peak displacement magnitude in world units at start.
    pub intensity: f32,
    /// New random offset samples per second.
    #[serde(default = "default_shake_freq")]
    pub frequency: f32,
    /// Total shake duration in seconds.
    pub duration: f32,
    /// Time elapsed inside this shake. Initialise to `0.0`.
    #[serde(default)]
    pub elapsed: f32,
    /// Offset applied on the last tick, removed before the next is applied.
    /// Runtime state; `None` until the first tick.
    ///
    /// Replaces a captured `origin` the entity was pinned to every tick, which
    /// froze anything moving - a knocked-back enemy, a walking player - for
    /// the length of the shake and then snapped it back to where the shake
    /// began. Scenes that serialized `origin` still load; it is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<[f32; 2]>,
    /// Internal PRNG state seeded on first tick — kept on the
    /// component so each entity has its own deterministic stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
}

fn default_shake_freq() -> f32 {
    30.0
}

impl Default for Shake {
    fn default() -> Self {
        Self {
            intensity: 4.0,
            frequency: 30.0,
            duration: 0.3,
            elapsed: 0.0,
            offset: None,
            seed: None,
        }
    }
}

