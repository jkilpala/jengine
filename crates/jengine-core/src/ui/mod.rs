//! In-game UI system — entity-based UI for HUD, text, buttons, panels, images.
//!
//! UI entities use screen-space positioning via anchor + offset rather than
//! world-space Transform. Rendered in a separate orthographic pass after the
//! game world (on top of everything except the debug overlay).
//!
//! Submodules:
//! - [`gui_document`] — tree-based `.gui` asset format for full-screen layouts
//! - [`layout`] — flex layout engine that resolves widget trees to screen rects

pub mod gui_document;
/// The UI as a screen reader sees it (`accessible`, `Accessibility.announce`).
pub mod accessibility;
pub mod drag_drop;
pub mod radial_menu;
pub mod speech_bubble;
pub mod toast;
pub mod layout;
pub mod navigation;
pub mod scaling;
pub mod scroll;
/// A text field you can edit: caret, selection, words, clipboard, IME.
pub mod text_edit;
pub mod shapes;

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Screen anchor point for UI elements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UIAnchor {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl UIAnchor {
    /// Compute the screen-space origin point for this anchor given screen dimensions.
    /// Returns (x, y) with (0,0) = top-left.
    pub fn origin(&self, screen_w: f32, screen_h: f32) -> Vec2 {
        match self {
            Self::TopLeft => Vec2::new(0.0, screen_h),
            Self::TopCenter => Vec2::new(screen_w * 0.5, screen_h),
            Self::TopRight => Vec2::new(screen_w, screen_h),
            Self::CenterLeft => Vec2::new(0.0, screen_h * 0.5),
            Self::Center => Vec2::new(screen_w * 0.5, screen_h * 0.5),
            Self::CenterRight => Vec2::new(screen_w, screen_h * 0.5),
            Self::BottomLeft => Vec2::new(0.0, 0.0),
            Self::BottomCenter => Vec2::new(screen_w * 0.5, 0.0),
            Self::BottomRight => Vec2::new(screen_w, 0.0),
        }
    }

    /// Parse an anchor name from a string, in either spelling: a script's
    /// `"top_right"` or a scene's `"TopRight"` (case, `_` and `-` are
    /// ignored). Until Oct 2026 only the first was known and anything else
    /// was silently `TopLeft` - so `UI.set_position(e, "TopRight", ..)`,
    /// copied from the scene that authored `e`, put it against the wrong
    /// edge with no error. An unknown name still falls back, with a warning.
    pub fn from_name(name: &str) -> Self {
        let key: String = name.chars().filter(|c| *c != '_' && *c != '-').collect::<String>().to_ascii_lowercase();
        match key.as_str() {
            "topleft" => Self::TopLeft,
            "topcenter" => Self::TopCenter,
            "topright" => Self::TopRight,
            "centerleft" => Self::CenterLeft,
            "center" => Self::Center,
            "centerright" => Self::CenterRight,
            "bottomleft" => Self::BottomLeft,
            "bottomcenter" => Self::BottomCenter,
            "bottomright" => Self::BottomRight,
            _ => {
                log::warn!("[ui] unknown anchor '{name}' - using top_left");
                Self::TopLeft
            }
        }
    }

    /// Returns (fx, fy) fractions for aligning a rect to this anchor.
    ///
    /// The fraction tells which point of the rect should coincide with the
    /// anchor point on screen: (0,0) = top-left, (0.5,0.5) = center, etc.
    pub fn alignment_fraction(&self) -> (f32, f32) {
        let fx = match self {
            Self::TopLeft | Self::CenterLeft | Self::BottomLeft => 0.0,
            Self::TopCenter | Self::Center | Self::BottomCenter => 0.5,
            Self::TopRight | Self::CenterRight | Self::BottomRight => 1.0,
        };
        let fy = match self {
            Self::TopLeft | Self::TopCenter | Self::TopRight => 0.0,
            Self::CenterLeft | Self::Center | Self::CenterRight => 0.5,
            Self::BottomLeft | Self::BottomCenter | Self::BottomRight => 1.0,
        };
        (fx, fy)
    }
}

/// Text alignment: within a `UIText` element's box, and for world text
/// (`TextComponent::align`), about the transform's x.
///
/// For world text, `Left` (the default, and how text always drew) starts each
/// line at x, `Center` centres each line on x and `Right` ends it there - each
/// line measured on its own. So a centred title stays centred when its string
/// changes, instead of being authored at `x = -width / 2` by hand and going
/// stale the next time someone edits it. Scenes may write it either way:
/// `"Center"` or `"center"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    #[serde(alias = "left")]
    Left,
    #[serde(alias = "center")]
    Center,
    #[serde(alias = "right")]
    Right,
}

impl TextAlign {
    /// How far left of its anchor a line of `width` starts.
    pub fn offset(self, width: f32) -> f32 {
        match self {
            TextAlign::Left => 0.0,
            TextAlign::Center => width * 0.5,
            TextAlign::Right => width,
        }
    }
}

/// Every property [`read_ui_prop`] answers - what the browser hosts publish
/// per widget for `get_ui_prop`, which there reads a snapshot rather than the
/// world. Each host kept its own copy of this list, so a property added to
/// the reader had to be remembered in two more places or it read `nil` in
/// the editor and on the web only.
pub const READABLE_UI_PROPS: &[&str] = &[
    "text", "value", "checked", "disabled", "pressed", "hovered", "focused", "visible",
    "texture_id", "region", "color", "bg_color", "submitted", "caret", "selected",
];

/// Read one property back off an entity-UI widget — the reader half of
/// [`apply_ui_prop`], sharing its property names.
///
/// Without this a script could write a slider but never learn where the user
/// dragged it, and `UIButton`'s `pressed` / `hovered` — which the engine
/// maintains every frame in
/// [`crate::ecs::systems::update_ui_widgets`] — were invisible, so a button
/// could not do anything. Polling `pressed` in `update` is the entity-UI
/// answer to `gui_on_click`.
///
/// Returns `None` when the entity has no widget carrying the property, which
/// reaches scripts as `nil` and stays distinguishable from a real `false` or
/// `0`.
pub fn read_ui_prop(
    world: &hecs::World,
    entity: hecs::Entity,
    property: &str,
) -> Option<serde_json::Value> {
    use serde_json::Value as J;
    let num = |f: f32| J::from(f as f64);
    let rgba = |c: [f32; 4]| J::Array(c.iter().map(|v| J::from(*v as f64)).collect());
    match property {
        "text" => world
            .get::<&UIText>(entity)
            .map(|w| J::String(w.text.clone()))
            .or_else(|_| world.get::<&UITextInput>(entity).map(|w| J::String(w.text.clone())))
            .ok(),
        "value" => world
            .get::<&UIProgressBar>(entity)
            .map(|w| num(w.value))
            .or_else(|_| world.get::<&UISlider>(entity).map(|w| num(w.value)))
            .ok(),
        "checked" => world.get::<&UIToggle>(entity).map(|w| J::Bool(w.checked)).ok(),
        "disabled" => world.get::<&UIButton>(entity).map(|w| J::Bool(w.disabled)).ok(),
        // Runtime state the button system writes each frame; this is what makes
        // a UIButton usable from a script at all.
        "pressed" => world.get::<&UIButton>(entity).map(|w| J::Bool(w.pressed)).ok(),
        "hovered" => world.get::<&UIButton>(entity).map(|w| J::Bool(w.hovered)).ok(),
        "focused" => world.get::<&UITextInput>(entity).map(|w| J::Bool(w.focused)).ok(),
        // A text field's: Enter pressed this frame, the caret (characters
        // from the start), the selected text.
        "submitted" => world.get::<&UITextInput>(entity).map(|w| J::Bool(w.edit.submitted)).ok(),
        "caret" => world.get::<&UITextInput>(entity).map(|w| J::from(w.edit.caret as u64)).ok(),
        "selected" => world.get::<&UITextInput>(entity).map(|w| J::String(w.edit.selected(&w.text).to_string())).ok(),
        "visible" => world.get::<&UIElement>(entity).map(|e| J::Bool(e.visible)).ok(),
        "texture_id" => world
            .get::<&UIImage>(entity)
            .map(|w| J::String(w.texture_id.clone()))
            .ok(),
        "region" => world
            .get::<&UIImage>(entity)
            .ok()
            .and_then(|w| w.region.clone())
            .map(J::String),
        "color" => world
            .get::<&UIText>(entity)
            .map(|w| rgba(w.color))
            .or_else(|_| world.get::<&UIImage>(entity).map(|w| rgba(w.color)))
            .ok(),
        "bg_color" => world
            .get::<&UIPanel>(entity)
            .map(|w| rgba(w.bg_color))
            .or_else(|_| world.get::<&UITextInput>(entity).map(|w| rgba(w.bg_color)))
            .ok(),
        // Keep [`READABLE_UI_PROPS`] in step with the arms above.
        _ => None,
    }
}

/// Apply one `set_ui_prop(entity, property, value)` write to an entity-UI
/// widget.
///
/// The twin of [`gui_document::apply_gui_node_prop`] for the *entity* UI
/// system, and for the same reason: one shared dispatch means a property
/// added here works in every runtime, instead of each host growing its own
/// copy that silently diverges.
///
/// Returns `false` for an unknown property so each caller can log in its own
/// voice, and `false` when the entity has no widget the property applies to —
/// setting `"checked"` on a Text is a no-op, not an error.
///
/// Widgets could previously only be authored: nothing could change a
/// ProgressBar's value, a Slider's position, a Toggle's state or a TextInput's
/// contents after scene load.
pub fn apply_ui_prop(
    world: &mut hecs::World,
    entity: hecs::Entity,
    property: &str,
    value: &serde_json::Value,
) -> bool {
    // Scripts hand values across the FFI as strings which the bridges re-parse
    // as JSON, so a number can arrive either way. Same coercion as the .gui
    // side.
    let as_f32 = |v: &serde_json::Value| -> Option<f32> {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
            .map(|f| f as f32)
    };
    let as_flag = |v: &serde_json::Value| -> Option<bool> {
        match v {
            serde_json::Value::Bool(b) => Some(*b),
            serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0),
            serde_json::Value::String(s) => match s.as_str() {
                "true" | "1" => Some(true),
                "false" | "0" | "" => Some(false),
                _ => None,
            },
            _ => None,
        }
    };
    let as_text = |v: &serde_json::Value| -> Option<String> {
        match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    };
    let as_color = |v: &serde_json::Value| -> Option<[f32; 4]> {
        serde_json::from_value::<[f32; 4]>(v.clone()).ok()
    };

    let mut hit = false;
    match property {
        "text" => {
            if let Some(t) = as_text(value) {
                if let Ok(mut w) = world.get::<&mut UIText>(entity) {
                    w.text = t.clone();
                    hit = true;
                }
                if let Ok(mut w) = world.get::<&mut UITextInput>(entity) {
                    w.text = t;
                    hit = true;
                }
            }
        }
        // A text field: focus it (the caret at the end, every other field
        // let go) or let it go; its limit, its mask, its prompt.
        "focused" => {
            if let Some(b) = as_flag(value)
                && world.get::<&UITextInput>(entity).is_ok()
            {
                // Focusing one field lets every other go; letting one go
                // touches only it.
                let fields: Vec<hecs::Entity> = if b {
                    world.query::<&UITextInput>().iter().map(|(e, _)| e).collect()
                } else {
                    vec![entity]
                };
                for e in fields {
                    if let Ok(mut w) = world.get::<&mut UITextInput>(e) {
                        let on = b && e == entity;
                        if on && !w.focused {
                            let text = w.text.clone();
                            w.edit.focus_end(&text);
                        }
                        w.focused = on;
                    }
                }
                hit = true;
            }
        }
        // The caret (a number) or the selection (`[start, end]`).
        "caret" | "selection" => {
            if let Ok(mut w) = world.get::<&mut UITextInput>(entity) {
                let w = &mut *w;
                hit = w.edit.set_from_script(&w.text, value);
            }
        }
        "max_length" => {
            if let Some(n) = as_f32(value)
                && let Ok(mut w) = world.get::<&mut UITextInput>(entity)
            {
                w.max_length = n.max(0.0) as usize;
                hit = true;
            }
        }
        "password" => {
            if let Some(b) = as_flag(value)
                && let Ok(mut w) = world.get::<&mut UITextInput>(entity)
            {
                w.password = b;
                hit = true;
            }
        }
        "placeholder" => {
            if let Some(t) = as_text(value)
                && let Ok(mut w) = world.get::<&mut UITextInput>(entity)
            {
                w.placeholder = t;
                hit = true;
            }
        }
        "value" => {
            if let Some(n) = as_f32(value) {
                if let Ok(mut w) = world.get::<&mut UIProgressBar>(entity) {
                    w.value = n.clamp(w.min, w.max);
                    hit = true;
                }
                if let Ok(mut w) = world.get::<&mut UISlider>(entity) {
                    w.value = n.clamp(w.min, w.max);
                    hit = true;
                }
            }
        }
        "checked" => {
            if let Some(b) = as_flag(value)
                && let Ok(mut w) = world.get::<&mut UIToggle>(entity)
            {
                w.checked = b;
                hit = true;
            }
        }
        "disabled" => {
            if let Some(b) = as_flag(value)
                && let Ok(mut w) = world.get::<&mut UIButton>(entity)
            {
                w.disabled = b;
                hit = true;
            }
        }
        "visible" => {
            if let Some(b) = as_flag(value)
                && let Ok(mut e) = world.get::<&mut UIElement>(entity)
            {
                e.visible = b;
                hit = true;
            }
        }
        // `{ w, h }` in UI pixels: a window that fits what it holds - a pack
        // of three items or of twenty - rather than one sized for the most.
        "size" => {
            if let Ok([w, h]) = serde_json::from_value::<[f32; 2]>(value.clone())
                && w.is_finite()
                && h.is_finite()
                && let Ok(mut e) = world.get::<&mut UIElement>(entity)
            {
                e.size = Vec2::new(w.max(0.0), h.max(0.0));
                hit = true;
            }
        }
        "texture_id" => {
            if let Some(t) = as_text(value)
                && let Ok(mut w) = world.get::<&mut UIImage>(entity)
            {
                w.texture_id = t;
                w.source_px = None;
                // A region names a rect *in this* texture: look it up again.
                if w.region.is_some() {
                    w.uv_rect = None;
                    w.region_resolved = false;
                }
                hit = true;
            }
        }
        "region" => {
            if let Some(r) = as_text(value)
                && let Ok(mut w) = world.get::<&mut UIImage>(entity)
            {
                if w.region.as_deref() != Some(r.as_str()) {
                    w.region = Some(r);
                    w.uv_rect = None;
                    w.region_resolved = false;
                    w.source_px = None;
                }
                hit = true;
            }
        }
        "color" => {
            if let Some(c) = as_color(value) {
                if let Ok(mut w) = world.get::<&mut UIText>(entity) {
                    w.color = c;
                    hit = true;
                }
                if let Ok(mut w) = world.get::<&mut UIImage>(entity) {
                    w.color = c;
                    hit = true;
                }
            }
        }
        "bg_color" => {
            if let Some(c) = as_color(value) {
                if let Ok(mut w) = world.get::<&mut UIPanel>(entity) {
                    w.bg_color = c;
                    hit = true;
                }
                if let Ok(mut w) = world.get::<&mut UITextInput>(entity) {
                    w.bg_color = c;
                    hit = true;
                }
            }
        }
        _ => return false,
    }
    hit
}

/// Core UI layout component — determines screen position and size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIElement {
    /// Which point of the screen `offset` is measured from.
    pub anchor: UIAnchor,
    /// Where the element sits, measured from the anchor point.
    ///
    /// The offset positions **the corner or edge the anchor names**, not the
    /// element's centre: a `TopLeft` element's offset is its top-left corner,
    /// a `BottomRight` element's is its bottom-right corner, and a `Center`
    /// element's is its centre. That is what
    /// [`UIAnchor::alignment_fraction`] has always described and what `.gui`
    /// documents have always done.
    ///
    /// Y is **up**, so a `TopLeft` element moves down the screen with a
    /// *negative* y — `(40, -120)` is 40 in from the left and 120 down from
    /// the top.
    pub offset: Vec2,
    /// Width and height in screen pixels.
    pub size: Vec2,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub z_order: i32,
    /// Whether a D-pad, stick or the arrow keys can move focus here
    /// (`ui::navigation`). Opt-in, so a HUD button does not start taking the
    /// directions the player is moving with.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub focusable: bool,
    /// Runtime state - not serialized. Set by `navigate_ui`; drawn as a ring.
    #[serde(skip)]
    pub focused: bool,
}

impl UIElement {
    /// The element's centre in the renderer's Y-up screen space, where
    /// `(0, 0)` is the bottom-left corner.
    ///
    /// The renderer and the pointer hit test both go through here. They used
    /// to compute it separately and *both* ignored
    /// [`UIAnchor::alignment_fraction`], treating `anchor + offset` as the
    /// centre - so a `TopLeft` element straddled the screen's corner and half
    /// of it hung off the edge, while the same anchor in a `.gui` document
    /// placed the corner. Scene 40's 420-wide panel at offset `(40, -120)`
    /// started 170px to the left of the screen (BUGS #60).
    pub fn screen_center(&self, screen_w: f32, screen_h: f32) -> Vec2 {
        let point = self.anchor.origin(screen_w, screen_h) + self.offset;
        let (fx, fy) = self.anchor.alignment_fraction();
        Vec2::new(
            point.x + (0.5 - fx) * self.size.x,
            // Y is up while `fy` counts down from the top, hence the sign.
            point.y - (0.5 - fy) * self.size.y,
        )
    }

    /// The element's screen rect as `(min, max)` corners, Y-up.
    pub fn screen_rect(&self, screen_w: f32, screen_h: f32) -> (Vec2, Vec2) {
        let center = self.screen_center(screen_w, screen_h);
        let half = self.size * 0.5;
        (center - half, center + half)
    }
}

fn default_true() -> bool {
    true
}

impl Default for UIElement {
    fn default() -> Self {
        Self {
            anchor: UIAnchor::TopLeft,
            offset: Vec2::ZERO,
            size: Vec2::new(100.0, 30.0),
            visible: true,
            z_order: 0,
            focusable: false,
            focused: false,
        }
    }
}

/// UI text rendering component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIText {
    pub text: String,
    #[serde(default)]
    pub font_id: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_white")]
    pub color: [f32; 4],
    #[serde(default)]
    pub alignment: TextAlign,
    /// An outline round the glyphs, as world text has
    /// ([`crate::ecs::components::TextOutline`]); its width in UI pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<crate::ecs::components::TextOutline>,
    /// A shadow under them ([`crate::ecs::components::TextShadow`]), in UI
    /// pixels, y up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<crate::ecs::components::TextShadow>,
    /// A string-table key, as on world text (`locale`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text_key: String,
    /// The `locale::stamp` the text was last written for. Never saved.
    #[serde(skip)]
    pub localized: u64,
    /// Read the text as markup, as world text's `rich` does
    /// (`renderer::rich_text`, Oct 2026): colours, bold, wave / shake /
    /// rainbow, pauses and inline icons.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rich: bool,
    /// A typewriter, characters a second from whenever `text` last changed
    /// (0 shows it all at once), as world text's.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reveal_speed: f32,
    /// The typewriter's clock, kept by `rich_text::tick_reveals`. Never saved.
    #[serde(skip)]
    pub reveal: Option<crate::ecs::components::TextReveal>,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn default_font_size() -> f32 {
    1.0
}

fn default_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

impl Default for UIText {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_id: String::new(),
            font_size: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            alignment: TextAlign::Left,
            outline: None,
            shadow: None,
            text_key: String::new(),
            localized: 0,
            rich: false,
            reveal_speed: 0.0,
            reveal: None,
        }
    }
}

/// UI image component — renders a textured quad.
///
/// Names an atlas `region` the way a sprite does, since Sep 2026. Before that
/// an icon cut from a sheet had to be addressed by a `uv_rect` computed by
/// hand from the atlas's pixel rects - which goes stale the moment the sheet
/// is repacked - and a script could not swap one icon for another at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIImage {
    pub texture_id: String,
    #[serde(default = "default_white")]
    pub color: [f32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_rect: Option<[f32; 4]>,
    /// A named region of the atlas `texture_id` points at; resolved into
    /// `uv_rect` by [`crate::ecs::systems::resolve_sprite_regions`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Runtime state - the region has been looked up. Cleared when the
    /// region or texture is re-pointed.
    #[serde(skip)]
    pub region_resolved: bool,
    /// Draw as a nine-slice: corners kept, edges and middle stretched to the
    /// element's size - a panel or button frame from a kit (jSuite writes
    /// these borders into its kits). `None` stretches the whole image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nine_slice: Option<UiNineSlice>,
    /// Runtime state - the pixel size of what the image samples (its region,
    /// or the whole texture), which the borders are measured against. Filled
    /// by [`crate::ecs::systems::resolve_sprite_regions`]; until then a
    /// nine-slice image draws stretched.
    #[serde(skip)]
    pub source_px: Option<[f32; 2]>,
}

/// A [`UIImage`]'s nine-slice: borders in **texture pixels**, drawn `scale`
/// UI pixels per texture pixel - so a 14-pixel pixel-art corner can be drawn
/// three times its size and stay crisp, which a border measured in UI
/// pixels (as a sprite's is, in world units) could not say.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UiNineSlice {
    #[serde(default)]
    pub left: f32,
    #[serde(default)]
    pub right: f32,
    #[serde(default)]
    pub top: f32,
    #[serde(default)]
    pub bottom: f32,
    #[serde(default = "one")]
    pub scale: f32,
}

fn one() -> f32 {
    1.0
}

impl Default for UIImage {
    fn default() -> Self {
        Self {
            texture_id: Default::default(),
            color: [1.0, 1.0, 1.0, 1.0],
            uv_rect: None,
            region: None,
            region_resolved: false,
            nine_slice: None,
            source_px: None,
        }
    }
}

/// UI panel component — a colored rectangle background.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIPanel {
    #[serde(default = "default_panel_bg")]
    pub bg_color: [f32; 4],
}

fn default_panel_bg() -> [f32; 4] {
    [0.2, 0.2, 0.2, 0.8]
}

impl Default for UIPanel {
    fn default() -> Self {
        Self {
            bg_color: [0.2, 0.2, 0.2, 0.8],
        }
    }
}

/// UI button component — interactive element with hover/pressed states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIButton {
    #[serde(default = "default_button_normal")]
    pub normal_color: [f32; 4],
    #[serde(default = "default_button_hover")]
    pub hover_color: [f32; 4],
    #[serde(default = "default_button_pressed")]
    pub pressed_color: [f32; 4],
    #[serde(default)]
    pub disabled: bool,
    /// Runtime state — not serialized.
    #[serde(skip)]
    pub hovered: bool,
    /// Runtime state — not serialized.
    #[serde(skip)]
    pub pressed: bool,
    /// Runtime state - not serialized. The press began on this button, so a
    /// release over it is a click.
    #[serde(skip)]
    pub armed: bool,
}

fn default_button_normal() -> [f32; 4] {
    [0.3, 0.3, 0.3, 1.0]
}

fn default_button_hover() -> [f32; 4] {
    [0.4, 0.4, 0.4, 1.0]
}

fn default_button_pressed() -> [f32; 4] {
    [0.2, 0.2, 0.2, 1.0]
}

impl Default for UIButton {
    fn default() -> Self {
        Self {
            normal_color: [0.3, 0.3, 0.3, 1.0],
            hover_color: [0.4, 0.4, 0.4, 1.0],
            pressed_color: [0.2, 0.2, 0.2, 1.0],
            disabled: false,
            hovered: false,
            pressed: false,
            armed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// New widget components (Phase 5 — GUI system)
// ---------------------------------------------------------------------------

fn default_progress_value() -> f32 {
    0.5
}

fn default_one() -> f32 {
    1.0
}

fn default_progress_fg() -> [f32; 4] {
    [0.2, 0.7, 0.3, 1.0]
}

fn default_progress_bg_color() -> [f32; 4] {
    [0.15, 0.15, 0.15, 1.0]
}

fn default_slider_track() -> [f32; 4] {
    [0.3, 0.3, 0.3, 1.0]
}

fn default_slider_thumb() -> [f32; 4] {
    [0.8, 0.8, 0.8, 1.0]
}

fn default_thumb_size() -> f32 {
    16.0
}

fn default_toggle_on() -> [f32; 4] {
    [0.2, 0.7, 0.3, 1.0]
}

fn default_toggle_off() -> [f32; 4] {
    [0.4, 0.4, 0.4, 1.0]
}

fn default_input_bg() -> [f32; 4] {
    [0.12, 0.12, 0.12, 1.0]
}

/// UI progress bar — horizontal fill indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIProgressBar {
    #[serde(default = "default_progress_value")]
    pub value: f32,
    #[serde(default)]
    pub min: f32,
    #[serde(default = "default_one")]
    pub max: f32,
    #[serde(default = "default_progress_fg")]
    pub fg_color: [f32; 4],
    #[serde(default = "default_progress_bg_color")]
    pub bg_color: [f32; 4],
    #[serde(default = "default_transparent")]
    pub border_color: [f32; 4],
}

fn default_transparent() -> [f32; 4] {
    [0.0, 0.0, 0.0, 0.0]
}

impl Default for UIProgressBar {
    fn default() -> Self {
        Self {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            fg_color: [0.2, 0.7, 0.3, 1.0],
            bg_color: [0.15, 0.15, 0.15, 1.0],
            border_color: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

/// UI slider — draggable value control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UISlider {
    #[serde(default = "default_progress_value")]
    pub value: f32,
    #[serde(default)]
    pub min: f32,
    #[serde(default = "default_one")]
    pub max: f32,
    #[serde(default = "default_slider_track")]
    pub track_color: [f32; 4],
    #[serde(default = "default_slider_thumb")]
    pub thumb_color: [f32; 4],
    #[serde(default = "default_thumb_size")]
    pub thumb_size: f32,
    /// Runtime state - not serialized.
    ///
    /// A drag belongs to the slider it started on and keeps the pointer until
    /// the button comes up, even once the cursor has left the track. Without
    /// that, dragging to either end - the two values people reach for most -
    /// would let go the moment you overshot.
    #[serde(skip)]
    pub dragging: bool,
}

impl Default for UISlider {
    fn default() -> Self {
        Self {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            track_color: [0.3, 0.3, 0.3, 1.0],
            thumb_color: [0.8, 0.8, 0.8, 1.0],
            thumb_size: 16.0,
            dragging: false,
        }
    }
}

/// UI toggle switch — on/off state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIToggle {
    #[serde(default)]
    pub checked: bool,
    #[serde(default = "default_toggle_on")]
    pub on_color: [f32; 4],
    #[serde(default = "default_toggle_off")]
    pub off_color: [f32; 4],
    #[serde(default = "default_white")]
    pub thumb_color: [f32; 4],
}

impl Default for UIToggle {
    fn default() -> Self {
        Self {
            checked: false,
            on_color: [0.2, 0.7, 0.3, 1.0],
            off_color: [0.4, 0.4, 0.4, 1.0],
            thumb_color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// UI text input — single-line editable text field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UITextInput {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub font_id: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_white")]
    pub color: [f32; 4],
    #[serde(default = "default_input_bg")]
    pub bg_color: [f32; 4],
    /// The most characters it takes; 0 is no limit of its own
    /// ([`text_edit::MAX_CHARS`]).
    #[serde(default)]
    pub max_length: usize,
    /// Shows a `*` for each character.
    #[serde(default)]
    pub password: bool,
    /// Runtime state — not serialized.
    #[serde(skip)]
    pub focused: bool,
    /// The caret, the selection and the rest (runtime; [`text_edit`]).
    #[serde(skip)]
    pub edit: text_edit::TextEdit,
}

impl Default for UITextInput {
    fn default() -> Self {
        Self {
            text: String::new(),
            placeholder: String::new(),
            font_id: String::new(),
            font_size: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            bg_color: [0.12, 0.12, 0.12, 1.0],
            max_length: 0,
            password: false,
            focused: false,
            edit: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_origins() {
        let w = 800.0;
        let h = 600.0;
        assert_eq!(UIAnchor::TopLeft.origin(w, h), Vec2::new(0.0, 600.0));
        assert_eq!(UIAnchor::Center.origin(w, h), Vec2::new(400.0, 300.0));
        assert_eq!(UIAnchor::BottomRight.origin(w, h), Vec2::new(800.0, 0.0));
        assert_eq!(UIAnchor::TopCenter.origin(w, h), Vec2::new(400.0, 600.0));
        assert_eq!(UIAnchor::BottomLeft.origin(w, h), Vec2::new(0.0, 0.0));
    }

    #[test]
    fn a_top_left_element_places_its_corner_not_its_centre() {
        // Scene 40's panel, the shape the bug was seen in: 40 in from the
        // left, 120 down from the top, 420x300.
        let panel = UIElement {
            anchor: UIAnchor::TopLeft,
            offset: Vec2::new(40.0, -120.0),
            size: Vec2::new(420.0, 300.0),
            ..Default::default()
        };
        let (min, max) = panel.screen_rect(1000.0, 800.0);
        assert_eq!(min.x, 40.0, "left edge sits at the offset");
        assert_eq!(max.x, 460.0);
        // Y is up: 120 down from the top is 800 - 120.
        assert_eq!(max.y, 680.0, "top edge sits at the offset");
        assert_eq!(min.y, 380.0);
        // Treating the offset as the centre put 170px of it off-screen.
        assert!(min.x >= 0.0);
    }

    #[test]
    fn a_zero_offset_element_sits_inside_the_screen_for_every_anchor() {
        const ANCHORS: [UIAnchor; 9] = [
            UIAnchor::TopLeft,
            UIAnchor::TopCenter,
            UIAnchor::TopRight,
            UIAnchor::CenterLeft,
            UIAnchor::Center,
            UIAnchor::CenterRight,
            UIAnchor::BottomLeft,
            UIAnchor::BottomCenter,
            UIAnchor::BottomRight,
        ];
        let (w, h) = (640.0, 480.0);
        for anchor in ANCHORS {
            let elem = UIElement {
                anchor,
                offset: Vec2::ZERO,
                size: Vec2::new(200.0, 60.0),
                ..Default::default()
            };
            let (min, max) = elem.screen_rect(w, h);
            assert!(
                min.x >= 0.0 && min.y >= 0.0 && max.x <= w && max.y <= h,
                "{anchor:?} at offset zero hangs off the screen: {min:?}..{max:?}"
            );
        }
    }

    #[test]
    fn opposite_anchors_mirror_each_other() {
        let size = Vec2::new(100.0, 40.0);
        let tl = UIElement { anchor: UIAnchor::TopLeft, size, ..Default::default() };
        let br = UIElement { anchor: UIAnchor::BottomRight, size, ..Default::default() };
        let (tl_min, tl_max) = tl.screen_rect(800.0, 600.0);
        let (br_min, br_max) = br.screen_rect(800.0, 600.0);
        assert_eq!(tl_min, Vec2::new(0.0, 560.0));
        assert_eq!(tl_max, Vec2::new(100.0, 600.0));
        assert_eq!(br_min, Vec2::new(700.0, 0.0));
        assert_eq!(br_max, Vec2::new(800.0, 40.0));
    }

    #[test]
    fn ui_element_serde_roundtrip() {
        let elem = UIElement {
            anchor: UIAnchor::Center,
            offset: Vec2::new(10.0, -20.0),
            size: Vec2::new(200.0, 50.0),
            visible: true,
            z_order: 5,
            ..Default::default()
        };
        let json = serde_json::to_string(&elem).unwrap();
        let parsed: UIElement = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.anchor, elem.anchor);
        assert_eq!(parsed.z_order, 5);
    }

    #[test]
    fn ui_text_defaults() {
        let text: UIText = serde_json::from_str(r#"{"text":"Hello"}"#).unwrap();
        assert_eq!(text.text, "Hello");
        assert_eq!(text.font_size, 1.0);
        assert_eq!(text.alignment, TextAlign::Left);
    }

    #[test]
    fn ui_button_serde_skips_runtime_state() {
        let mut btn = UIButton::default();
        btn.hovered = true;
        btn.pressed = true;
        let json = serde_json::to_string(&btn).unwrap();
        let parsed: UIButton = serde_json::from_str(&json).unwrap();
        assert!(!parsed.hovered);
        assert!(!parsed.pressed);
    }

    #[test]
    fn ui_progress_bar_roundtrip() {
        let bar = UIProgressBar {
            value: 0.75,
            min: 0.0,
            max: 100.0,
            ..Default::default()
        };
        let json = serde_json::to_string(&bar).unwrap();
        let parsed: UIProgressBar = serde_json::from_str(&json).unwrap();
        assert!((parsed.value - 0.75).abs() < f32::EPSILON);
        assert!((parsed.max - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ui_slider_defaults() {
        let slider: UISlider = serde_json::from_str("{}").unwrap();
        assert!((slider.value - 0.5).abs() < f32::EPSILON);
        assert!((slider.thumb_size - 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ui_toggle_roundtrip() {
        let toggle = UIToggle {
            checked: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&toggle).unwrap();
        let parsed: UIToggle = serde_json::from_str(&json).unwrap();
        assert!(parsed.checked);
    }

    #[test]
    fn ui_text_input_serde_skips_focused() {
        let mut input = UITextInput {
            text: "hello".to_string(),
            placeholder: "Type...".to_string(),
            ..Default::default()
        };
        input.focused = true;
        let json = serde_json::to_string(&input).unwrap();
        let parsed: UITextInput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.text, "hello");
        assert_eq!(parsed.placeholder, "Type...");
        assert!(!parsed.focused);
    }
}
