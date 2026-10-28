//! GUI document format — tree-based UI layouts for HUDs, menus, and overlays.
//!
//! A `.gui` file stores a `GuiDocument` — a tree of `GuiNode`s describing a
//! complete UI screen. Each node has a type (Panel, Text, Button, …), styling
//! properties, flex layout rules, and optional children.

use serde::{Deserialize, Serialize};

use super::UIAnchor;

// ---------------------------------------------------------------------------
// Size values
// ---------------------------------------------------------------------------

/// How a dimension (width or height) is resolved during layout.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
#[derive(Default)]
pub enum SizeValue {
    /// Fixed pixel size.
    Fixed(f32),
    /// Percentage of the parent's content area, **per hundred**: `50.0` is
    /// half, `100.0` is full.
    ///
    /// It used to be a 0–1 fraction while [`ConstraintValue::Percent`] — the
    /// other percentage in this same layout system — was already per-hundred,
    /// and the GUI editor authored *and* previewed this one as per-hundred
    /// too. So a document authored in the editor with `50%` stored `50.0` and
    /// the runtime laid it out fifty times too large, while hand-written
    /// documents using `1.0` for "full" worked. Two conventions for one word,
    /// with the authoring tool on the opposite side from the engine
    /// (BUGS #10).
    Percent(f32),
    /// Automatically sized to fit children.
    #[default]
    Auto,
}


// ---------------------------------------------------------------------------
// Flex layout
// ---------------------------------------------------------------------------

/// Flex direction — how children are arranged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlexDirection {
    #[default]
    Column,
    Row,
}

/// Cross-axis alignment for flex children.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlignItems {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

/// Main-axis content distribution for flex children.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JustifyContent {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
}

/// How a container treats children that stick out of its content box.
///
/// The GUI system had no notion of overflow before Sep 2026 — a column of
/// children taller than its parent simply drew over whatever was around it,
/// which is what the Engine Test Lab hub looked like with 30+ buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Overflow {
    /// Children draw outside the parent freely (the historical behaviour).
    #[default]
    Visible,
    /// Children are clipped to the parent's content box.
    Hidden,
    /// Children are clipped *and* shifted by `scroll_x` / `scroll_y`, which
    /// the engine drives from the mouse wheel and scripts can set via
    /// `gui_set(gui, node, "scroll_y", n)`.
    Scroll,
}

impl Overflow {
    /// True for the default (no clipping) — used to keep `.gui` files clean.
    pub fn is_visible(&self) -> bool {
        matches!(self, Self::Visible)
    }

    /// True when children must be clipped to the content box.
    pub fn clips(&self) -> bool {
        !self.is_visible()
    }

    /// True when the container accepts a scroll offset.
    pub fn scrolls(&self) -> bool {
        matches!(self, Self::Scroll)
    }
}

/// Flex layout configuration for a container node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlexLayout {
    #[serde(default)]
    pub direction: FlexDirection,
    #[serde(default)]
    pub gap: f32,
    #[serde(default)]
    pub align_items: AlignItems,
    #[serde(default)]
    pub justify_content: JustifyContent,
}

impl Default for FlexLayout {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Column,
            gap: 0.0,
            align_items: AlignItems::Start,
            justify_content: JustifyContent::Start,
        }
    }
}

// ---------------------------------------------------------------------------
// Grid layout
// ---------------------------------------------------------------------------

/// How a grid track (column or row) is sized.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum GridTrack {
    /// Fixed pixel size.
    Fixed(f32),
    /// Fractional unit — shares remaining space proportionally (like CSS `fr`).
    Fraction(f32),
    /// Automatically sized to fit content.
    Auto,
}

impl Default for GridTrack {
    fn default() -> Self {
        Self::Fraction(1.0)
    }
}

/// Grid layout configuration for a container node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridLayout {
    /// Column track definitions. Children are placed left-to-right, top-to-bottom.
    #[serde(default = "default_grid_tracks")]
    pub columns: Vec<GridTrack>,
    /// Row track definitions. Extra rows use `Auto` sizing.
    #[serde(default = "default_grid_tracks")]
    pub rows: Vec<GridTrack>,
    /// Gap between columns (horizontal).
    #[serde(default)]
    pub column_gap: f32,
    /// Gap between rows (vertical).
    #[serde(default)]
    pub row_gap: f32,
}

fn default_grid_tracks() -> Vec<GridTrack> {
    vec![GridTrack::Fraction(1.0)]
}

impl Default for GridLayout {
    fn default() -> Self {
        Self {
            columns: default_grid_tracks(),
            rows: default_grid_tracks(),
            column_gap: 0.0,
            row_gap: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Layout mode
// ---------------------------------------------------------------------------

/// How a container node arranges its children — either flex or grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum LayoutMode {
    /// Flex layout (row or column).
    Flex(FlexLayout),
    /// Grid layout (rows × columns).
    Grid(GridLayout),
}

impl Default for LayoutMode {
    fn default() -> Self {
        Self::Flex(FlexLayout::default())
    }
}

// ---------------------------------------------------------------------------
// Edge insets (padding / margin)
// ---------------------------------------------------------------------------

/// Four-sided inset values (top, right, bottom, left).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct EdgeInsets {
    #[serde(default)]
    pub top: f32,
    #[serde(default)]
    pub right: f32,
    #[serde(default)]
    pub bottom: f32,
    #[serde(default)]
    pub left: f32,
}

impl EdgeInsets {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    pub fn all(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    /// Total horizontal inset (left + right).
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// Total vertical inset (top + bottom).
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

// ---------------------------------------------------------------------------
// Constraint values (edge-pinning)
// ---------------------------------------------------------------------------

/// A distance value for edge constraints — either fixed pixels or a percentage
/// of the parent's dimension.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ConstraintValue {
    /// Fixed pixel distance from the edge.
    Px(f32),
    /// Percentage of the parent's width (for left/right) or height (for top/bottom).
    Percent(f32),
}

impl ConstraintValue {
    /// Resolve this constraint value against a parent dimension.
    pub fn resolve(&self, parent_extent: f32) -> f32 {
        match self {
            Self::Px(v) => *v,
            Self::Percent(p) => parent_extent * p / 100.0,
        }
    }
}

/// Edge-pinning constraints — each edge can optionally be pinned to its parent
/// at a fixed or percentage distance. When opposite edges are both pinned,
/// the node's size on that axis is derived (stretched).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Constraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<ConstraintValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<ConstraintValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<ConstraintValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<ConstraintValue>,
}

impl Constraints {
    /// Returns true if no edges are pinned (free positioning).
    pub fn is_empty(&self) -> bool {
        self.left.is_none() && self.right.is_none() && self.top.is_none() && self.bottom.is_none()
    }
}

// ---------------------------------------------------------------------------
// Node style
// ---------------------------------------------------------------------------

fn default_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_transparent() -> [f32; 4] {
    [0.0, 0.0, 0.0, 0.0]
}

fn default_true() -> bool {
    true
}

fn is_false(v: &bool) -> bool {
    !v
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

/// Visual and layout style properties shared by all node types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStyle {
    #[serde(default)]
    pub width: SizeValue,
    #[serde(default)]
    pub height: SizeValue,
    #[serde(default)]
    pub padding: EdgeInsets,
    #[serde(default)]
    pub margin: EdgeInsets,
    #[serde(default = "default_transparent")]
    pub bg_color: [f32; 4],
    #[serde(default = "default_transparent")]
    pub border_color: [f32; 4],
    #[serde(default)]
    pub border_width: f32,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub z_order: i32,
    /// Flex layout rules (how this node arranges its children).
    #[serde(default)]
    pub flex: FlexLayout,
    /// Layout mode: Flex (default) or Grid. When present, overrides `flex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutMode>,
    /// Grid column placement (0-based). Only used when the parent uses Grid layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_column: Option<usize>,
    /// Grid row placement (0-based). Only used when the parent uses Grid layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_row: Option<usize>,
    /// Manual X offset applied after layout (pixels). Use for fine-tuning position.
    #[serde(default)]
    pub offset_x: f32,
    /// Manual Y offset applied after layout (pixels). Use for fine-tuning position.
    #[serde(default)]
    pub offset_y: f32,
    /// Edge-pinning constraints. When set, the node is positioned absolutely
    /// relative to its parent rather than participating in flex/grid flow.
    #[serde(default, skip_serializing_if = "Constraints::is_empty")]
    pub constraints: Constraints,
    /// Whether this node can receive keyboard focus via Tab navigation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub focusable: bool,
    /// Tab order for keyboard navigation. Lower values are focused first.
    /// Nodes with the same tab_order are visited in document order.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub tab_order: i32,
    /// How children that exceed this node's content box are treated.
    #[serde(default, skip_serializing_if = "Overflow::is_visible")]
    pub overflow: Overflow,
    /// Horizontal scroll offset in pixels, applied to children when
    /// `overflow` is `Scroll`. Clamped to the scrollable range by layout.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub scroll_x: f32,
    /// Vertical scroll offset in pixels, applied to children when
    /// `overflow` is `Scroll`. Clamped to the scrollable range by layout.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub scroll_y: f32,
    /// Elastic stretch past the scrollable range, in pixels. Runtime only.
    ///
    /// Layout adds this *after* clamping `scroll_x`, which is what lets a
    /// pointer drag pull the content past its end and spring back without
    /// disturbing the authored offset. `scroll_x` stays the honest position —
    /// a script writing `99999` to mean "the end" still relies on the clamp.
    ///
    /// Never serialized: a stretch is a moment in a gesture, not a property of
    /// a document, and saving one would reopen a `.gui` mid-bounce. Driven by
    /// [`crate::ui::scroll::GuiScrollState`].
    #[serde(skip)]
    pub overscroll_x: f32,
    /// Elastic stretch past the scrollable range, in pixels. Runtime only —
    /// see [`Self::overscroll_x`].
    #[serde(skip)]
    pub overscroll_y: f32,
    /// Rounded corners, in pixels: the background, the border, a button, a
    /// progress bar and the shadow all follow them (Sep 2026).
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub corner_radius: f32,
    /// 0..1, and **inherited**: a panel at 0.5 draws everything in it at
    /// half. How a whole menu fades in without a script touching each node.
    #[serde(default = "default_one", skip_serializing_if = "is_one_f32")]
    pub opacity: f32,
    /// Scale about the node's centre, **inherited** by everything in it - a
    /// button that pops, a panel that grows in. Applied by layout, so what
    /// a click hits is where the node is drawn.
    #[serde(default = "default_one", skip_serializing_if = "is_one_f32")]
    pub scale: f32,
    /// A soft drop shadow under the node's background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<GuiShadow>,
    /// How a screen reader reads the node (`ui::accessibility`): a name
    /// where its id is all it has (a toggle, a slider), a description, or
    /// `hidden` for decoration. Text and buttons read their own words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessible: Option<crate::ui::Accessible>,
}

/// A soft shadow under a node: its colour, how far it is offset (x right,
/// y down, in pixels), and how far its edge fades (`blur`, pixels).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiShadow {
    pub color: [f32; 4],
    pub offset: [f32; 2],
    pub blur: f32,
}

impl Default for GuiShadow {
    fn default() -> Self {
        Self { color: [0.0, 0.0, 0.0, 0.45], offset: [0.0, 6.0], blur: 14.0 }
    }
}

fn is_one_f32(v: &f32) -> bool {
    *v == 1.0
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            width: SizeValue::Auto,
            height: SizeValue::Auto,
            padding: EdgeInsets::ZERO,
            margin: EdgeInsets::ZERO,
            bg_color: [0.0, 0.0, 0.0, 0.0],
            border_color: [0.0, 0.0, 0.0, 0.0],
            border_width: 0.0,
            visible: true,
            z_order: 0,
            flex: FlexLayout::default(),
            layout: None,
            grid_column: None,
            grid_row: None,
            offset_x: 0.0,
            offset_y: 0.0,
            constraints: Constraints::default(),
            focusable: false,
            tab_order: 0,
            overflow: Overflow::Visible,
            scroll_x: 0.0,
            scroll_y: 0.0,
            overscroll_x: 0.0,
            overscroll_y: 0.0,
            corner_radius: 0.0,
            opacity: 1.0,
            scale: 1.0,
            shadow: None,
            accessible: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Widget-specific data
// ---------------------------------------------------------------------------

/// Text alignment within a text node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuiTextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl GuiTextAlign {
    fn is_left(&self) -> bool {
        *self == GuiTextAlign::Left
    }
}

/// The concrete widget type and its specific properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[derive(Default)]
pub enum GuiNodeType {
    /// Plain container / colored rectangle.
    #[default]
    Panel,
    /// Static text label.
    Text {
        #[serde(default)]
        text: String,
        #[serde(default)]
        font_id: String,
        #[serde(default = "default_gui_font_size")]
        font_size: f32,
        #[serde(default = "default_white")]
        color: [f32; 4],
        #[serde(default)]
        alignment: GuiTextAlign,
        /// Read `text` as markup (`renderer::rich_text`, Oct 2026), as world
        /// text's `rich` does - after `{{variable}}` substitution, so a
        /// value can be coloured by the markup around it.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rich: bool,
    },
    /// Textured image.
    Image {
        #[serde(default)]
        texture_id: String,
        #[serde(default = "default_white")]
        tint: [f32; 4],
        /// Optional UV sub-rect: [u_min, v_min, u_max, v_max].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uv_rect: Option<[f32; 4]>,
    },
    /// Clickable button (label text rendered inside).
    Button {
        #[serde(default)]
        label: String,
        #[serde(default)]
        font_id: String,
        #[serde(default = "default_gui_font_size")]
        font_size: f32,
        #[serde(default = "default_white")]
        text_color: [f32; 4],
        #[serde(default = "default_button_normal")]
        normal_color: [f32; 4],
        #[serde(default = "default_button_hover")]
        hover_color: [f32; 4],
        #[serde(default = "default_button_pressed")]
        pressed_color: [f32; 4],
        #[serde(default)]
        disabled: bool,
        /// Where the label sits: left (the default, and all there was before
        /// Sep 2026), centre or right.
        #[serde(default, skip_serializing_if = "GuiTextAlign::is_left")]
        alignment: GuiTextAlign,
        /// How much bigger the button draws while hovered - eased in, with
        /// its colour, over a tenth of a second. 1 (the default) does not
        /// grow. Drawn, not laid out: the pointer is already on it.
        #[serde(default = "default_one", skip_serializing_if = "is_one_f32")]
        hover_scale: f32,
    },
    /// Horizontal progress bar.
    ProgressBar {
        #[serde(default = "default_half")]
        value: f32,
        #[serde(default)]
        min: f32,
        #[serde(default = "default_one")]
        max: f32,
        #[serde(default = "default_progress_fg")]
        fg_color: [f32; 4],
        #[serde(default = "default_progress_bg")]
        bg_color: [f32; 4],
        #[serde(default = "default_transparent")]
        border_color: [f32; 4],
    },
    /// Draggable slider.
    Slider {
        #[serde(default = "default_half")]
        value: f32,
        #[serde(default)]
        min: f32,
        #[serde(default = "default_one")]
        max: f32,
        #[serde(default = "default_slider_track")]
        track_color: [f32; 4],
        #[serde(default = "default_slider_thumb")]
        thumb_color: [f32; 4],
        #[serde(default = "default_thumb_size")]
        thumb_size: f32,
    },
    /// On/off toggle switch.
    Toggle {
        #[serde(default)]
        checked: bool,
        #[serde(default = "default_toggle_on")]
        on_color: [f32; 4],
        #[serde(default = "default_toggle_off")]
        off_color: [f32; 4],
        #[serde(default = "default_white")]
        thumb_color: [f32; 4],
    },
    /// Editable single-line text input.
    TextInput {
        #[serde(default)]
        text: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default)]
        font_id: String,
        #[serde(default = "default_gui_font_size")]
        font_size: f32,
        #[serde(default = "default_white")]
        color: [f32; 4],
        #[serde(default = "default_input_bg")]
        bg_color: [f32; 4],
        /// The most characters it takes (0: no limit of its own).
        #[serde(default, skip_serializing_if = "is_zero_usize")]
        max_length: usize,
        /// Shows a `*` for each character.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        password: bool,
        /// The caret, the selection and the rest (runtime;
        /// `ui::text_edit`).
        #[serde(skip)]
        edit: crate::ui::text_edit::TextEdit,
    },
    /// Circle / ellipse shape.
    Circle {
        #[serde(default = "default_white")]
        fill_color: [f32; 4],
        #[serde(default = "default_transparent")]
        stroke_color: [f32; 4],
        #[serde(default)]
        stroke_width: f32,
    },
}

fn is_zero_usize(n: &usize) -> bool {
    *n == 0
}

fn default_gui_font_size() -> f32 {
    16.0
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

fn default_half() -> f32 {
    0.5
}

fn default_one() -> f32 {
    1.0
}

fn default_progress_fg() -> [f32; 4] {
    [0.2, 0.7, 0.3, 1.0]
}

fn default_progress_bg() -> [f32; 4] {
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


// ---------------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------------

/// A single node in the GUI widget tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiNode {
    /// Unique identifier within the document (e.g. "health_bar", "score_label").
    pub id: String,
    /// Widget type and its specific properties.
    #[serde(default)]
    pub node_type: GuiNodeType,
    /// Visual and layout style.
    #[serde(default)]
    pub style: NodeStyle,
    /// Child nodes (for containers like Panel).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<GuiNode>,
}

impl GuiNode {
    /// Create a new panel node with the given id.
    pub fn new_panel(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle::default(),
            children: Vec::new(),
        }
    }

    /// Find a node by id in this subtree (depth-first).
    pub fn find_node(&self, id: &str) -> Option<&GuiNode> {
        if self.id == id {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find_node(id) {
                return Some(found);
            }
        }
        None
    }

    /// True when this node refuses clicks: a disabled button.
    ///
    /// Every runtime hit-tests `.gui` clicks itself, so this is the one
    /// place that decides. Without it `disabled` was a field that
    /// serialized, loaded and did nothing at all.
    pub fn refuses_clicks(&self) -> bool {
        matches!(self.node_type, GuiNodeType::Button { disabled: true, .. })
    }

    /// Find a mutable node by id in this subtree (depth-first).
    pub fn find_node_mut(&mut self, id: &str) -> Option<&mut GuiNode> {
        if self.id == id {
            return Some(self);
        }
        for child in &mut self.children {
            if let Some(found) = child.find_node_mut(id) {
                return Some(found);
            }
        }
        None
    }

    /// Every texture this node and its children draw: Image nodes' and the
    /// inline icons of rich Text nodes (Oct 2026). A host that preloads a
    /// document's textures walks this; one that only knew Images drew a
    /// rich label's icons as nothing.
    pub fn texture_ids(&self, out: &mut std::collections::BTreeSet<String>) {
        match &self.node_type {
            GuiNodeType::Image { texture_id, .. } if !texture_id.is_empty() => {
                out.insert(texture_id.clone());
            }
            GuiNodeType::Text { text, rich: true, .. } if text.contains("[icon=") => {
                for icon in crate::renderer::rich_text::parse(text, 1).icons {
                    out.insert(text[icon.id].to_string());
                }
            }
            _ => {}
        }
        for child in &self.children {
            child.texture_ids(out);
        }
    }

    /// Collect all focusable, visible node IDs in tab order.
    /// Sorted by (tab_order, document order). Used for Tab key navigation.
    pub fn collect_focusable(&self) -> Vec<String> {
        let mut result: Vec<(i32, usize, String)> = Vec::new();
        Self::collect_focusable_inner(self, &mut result, &mut 0);
        result.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        result.into_iter().map(|(_, _, id)| id).collect()
    }

    fn collect_focusable_inner(node: &GuiNode, out: &mut Vec<(i32, usize, String)>, counter: &mut usize) {
        if !node.style.visible {
            return;
        }
        if node.style.focusable {
            out.push((node.style.tab_order, *counter, node.id.clone()));
        }
        *counter += 1;
        for child in &node.children {
            Self::collect_focusable_inner(child, out, counter);
        }
    }
}

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

/// A complete GUI layout document — stored as a `.gui` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiDocument {
    /// Human-readable name (e.g. "Main HUD", "Pause Menu").
    pub name: String,
    /// Screen anchor for the root node.
    #[serde(default)]
    pub anchor: UIAnchor,
    /// The root widget tree.
    pub root: GuiNode,
    /// Whether the arrow keys, a D-pad or a stick walk this document's
    /// `focusable` nodes and A / Enter / Space press them
    /// (`ecs::systems::navigate_ui`). `false` for a document whose script
    /// runs its own cursor - the arcade's `lib/menu.lua` - which would
    /// otherwise have a second cursor moving beside it and a press landing
    /// on a different row.
    #[serde(default = "default_navigable", skip_serializing_if = "is_navigable")]
    pub navigable: bool,
}

fn default_navigable() -> bool {
    true
}

fn is_navigable(v: &bool) -> bool {
    *v
}

impl GuiDocument {
    /// True when a click on this node must be ignored — the node is a
    /// disabled button, or it is not in the document at all.
    ///
    /// Each runtime hit-tests clicks against its own copy of the layout, so
    /// each one asks this rather than reading `disabled` itself.
    pub fn refuses_clicks(&self, node_id: &str) -> bool {
        self.root.find_node(node_id).is_none_or(|n| n.refuses_clicks())
    }

    /// The id of the nearest scrolling container `node_id` sits in, if any -
    /// the one to scroll when focus moves to a row out of its view.
    pub fn scroll_container_of(&self, node_id: &str) -> Option<String> {
        fn walk<'a>(node: &'a GuiNode, id: &str, scrollers: &mut Vec<&'a GuiNode>) -> Option<String> {
            if node.id == id {
                return scrollers.last().map(|n| n.id.clone());
            }
            let pushed = node.style.overflow.scrolls();
            if pushed {
                scrollers.push(node);
            }
            for child in &node.children {
                if let Some(found) = walk(child, id, scrollers) {
                    return Some(found);
                }
            }
            if pushed {
                scrollers.pop();
            }
            None
        }
        walk(&self.root, node_id, &mut Vec::new())
    }
}

impl Default for GuiDocument {
    fn default() -> Self {
        Self {
            name: "Untitled GUI".to_string(),
            navigable: true,
            anchor: UIAnchor::TopLeft,
            root: GuiNode {
                id: "root".to_string(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    width: SizeValue::Percent(100.0),
                    height: SizeValue::Percent(100.0),
                    bg_color: [0.0, 0.0, 0.0, 0.0],
                    ..Default::default()
                },
                children: Vec::new(),
            },
        }
    }
}

/// Serialize a `GuiDocument` to a JSON string.
pub fn save_gui(doc: &GuiDocument) -> String {
    serde_json::to_string_pretty(doc).expect("GuiDocument should always serialize")
}

/// Deserialize a `GuiDocument` from a JSON string.
pub fn load_gui(json: &str) -> anyhow::Result<GuiDocument> {
    let doc: GuiDocument = serde_json::from_str(json)?;
    Ok(doc)
}

/// What a `gui_set` value means, from the string the script bridges carry
/// (both flatten it to text at their boundary). Most properties read it as
/// JSON, so numbers, flags and colours arrive typed. A **text** property keeps
/// the script's string as it was written: parsed, `"28.00"` became the number
/// 28 and drew as "28.0", and `"007"` as "7". Every runtime goes through this.
pub fn gui_prop_value(property: &str, raw: String) -> serde_json::Value {
    if matches!(property, "text" | "placeholder") {
        return serde_json::Value::String(raw);
    }
    serde_json::from_str::<serde_json::Value>(&raw).unwrap_or(serde_json::Value::String(raw))
}

/// Apply one `gui_set(gui_id, node_id, property, value)` write to a node.
///
/// Returns `false` if the property name is unknown, so each caller can log in
/// its own voice. This lives here, and not in the callers, because the native
/// apply-handler and the editor viewport each used to carry their own copy of
/// this match — which meant a property added to one was silently an "unknown
/// property" in the other. Every runtime must go through this function.
pub fn apply_gui_node_prop(
    node: &mut GuiNode,
    property: &str,
    value: &serde_json::Value,
) -> bool {
    // Scripts hand `gui_set` a stringified value which the bridges parse as
    // JSON when they can. That means a caller writing a number-shaped label —
    // `gui_set(gui, "score", "text", "42")`, straight out of the docs — arrives
    // as a JSON number, not a string. Render such values rather than dropping
    // the write.
    let as_text = |v: &serde_json::Value| -> Option<String> {
        match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    };
    // Likewise for flags: "true"/"false" parse to bools, but "1"/"0" parse to
    // numbers, and some callers send those.
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
    match property {
        "text" => {
            if let Some(s) = as_text(value) {
                let s = s.as_str();
                match &mut node.node_type {
                    GuiNodeType::Text { text, .. } => *text = s.to_string(),
                    GuiNodeType::Button { label, .. } => *label = s.to_string(),
                    // A text input's `text` is what the player typed and
                    // what `gui_get` reads back, so that is what `text`
                    // has to write. It used to write the *placeholder* -
                    // which looks identical on an empty field, so
                    // "clear the box" appeared to work right up until
                    // somebody typed in it (BUGS #138).
                    GuiNodeType::TextInput { text, .. } => *text = s.to_string(),
                    _ => {}
                }
            }
        }
        "placeholder" => {
            if let Some(s) = as_text(value)
                && let GuiNodeType::TextInput { placeholder, .. } = &mut node.node_type
            {
                *placeholder = s;
            }
        }
        "caret" | "selection" => {
            if let GuiNodeType::TextInput { text, edit, .. } = &mut node.node_type {
                edit.set_from_script(text, value);
            }
        }
        "max_length" => {
            if let Some(n) = value.as_f64().or_else(|| value.as_str().and_then(|s| s.parse().ok()))
                && let GuiNodeType::TextInput { max_length, .. } = &mut node.node_type
            {
                *max_length = n.max(0.0) as usize;
            }
        }
        "password" => {
            if let GuiNodeType::TextInput { password, .. } = &mut node.node_type {
                *password = matches!(value, serde_json::Value::Bool(true))
                    || value.as_f64().is_some_and(|n| n != 0.0)
                    || value.as_str() == Some("true");
            }
        }
        "value" => {
            if let Some(n) = value.as_f64() {
                let n = n as f32;
                match &mut node.node_type {
                    GuiNodeType::ProgressBar { value: v, .. } => *v = n,
                    GuiNodeType::Slider { value: v, .. } => *v = n,
                    _ => {}
                }
            }
        }
        "checked" => {
            if let GuiNodeType::Toggle { checked, .. } = &mut node.node_type
                && let Some(b) = as_flag(value)
            {
                *checked = b;
            }
        }
        // A shop is the reason this exists: a button for something the player
        // cannot afford should say so and refuse the click, and every game
        // that wanted it had to hide the button instead.
        "disabled" => {
            if let Some(b) = as_flag(value)
                && let GuiNodeType::Button { disabled, .. } = &mut node.node_type
            {
                *disabled = b;
            }
        }
        "normal_color" | "hover_color" | "pressed_color" => {
            if let Ok(c) = serde_json::from_value::<[f32; 4]>(value.clone())
                && let GuiNodeType::Button {
                    normal_color,
                    hover_color,
                    pressed_color,
                    ..
                } = &mut node.node_type
            {
                match property {
                    "normal_color" => *normal_color = c,
                    "hover_color" => *hover_color = c,
                    _ => *pressed_color = c,
                }
            }
        }
        "visible" => {
            if let Some(b) = as_flag(value) {
                node.style.visible = b;
            }
        }
        "constraint_left" | "constraint_right" | "constraint_top" | "constraint_bottom" => {
            let cv: Option<ConstraintValue> = if value.is_null() {
                None
            } else {
                serde_json::from_value(value.clone()).ok()
            };
            match property {
                "constraint_left" => node.style.constraints.left = cv,
                "constraint_right" => node.style.constraints.right = cv,
                "constraint_top" => node.style.constraints.top = cv,
                "constraint_bottom" => node.style.constraints.bottom = cv,
                _ => unreachable!(),
            }
        }
        "focusable" => {
            if let Some(b) = as_flag(value) {
                node.style.focusable = b;
            }
        }
        "tab_order" => {
            if let Some(n) = value.as_i64() {
                node.style.tab_order = n as i32;
            }
        }
        // Swapping which image a node shows and recolouring it are what an
        // inventory panel is made of: one slot node whose icon and rarity tint
        // change as items move around.
        "texture_id" => {
            if let Some(t) = value.as_str()
                && let GuiNodeType::Image { texture_id, .. } = &mut node.node_type
            {
                *texture_id = t.to_string();
            }
        }
        "uv_rect" => {
            if let GuiNodeType::Image { uv_rect, .. } = &mut node.node_type {
                *uv_rect = if value.is_null() {
                    None
                } else {
                    serde_json::from_value(value.clone()).ok()
                };
                // The format is [u_min, v_min, u_max, v_max]. [x, y, width,
                // height] - the other obvious way to describe a cell - usually
                // gives a max at or below its min, which draws a mirrored,
                // zero-height smear with no error anywhere. A game's
                // upgrade cards shipped exactly that.
                if let Some([u0, v0, u1, v1]) = *uv_rect
                    && (u1 <= u0 || v1 <= v0)
                {
                    log::warn!(
                        "gui_set: uv_rect {:?} on node '{}' has a zero or negative span -                          it is [u_min, v_min, u_max, v_max], not [x, y, width, height]",
                        [u0, v0, u1, v1],
                        node.id
                    );
                }
            }
        }
        "tint" | "color" => {
            if let Ok(c) = serde_json::from_value::<[f32; 4]>(value.clone()) {
                match &mut node.node_type {
                    GuiNodeType::Image { tint, .. } => *tint = c,
                    GuiNodeType::Text { color, .. } => *color = c,
                    GuiNodeType::Button { text_color, .. } => *text_color = c,
                    _ => {}
                }
            }
        }
        // A Button draws its face from `normal_color` and never reads its
        // style's `bg_color`, so on a Button this is the face (Oct 2026):
        // three Strategy Classics games set it, got `true` back and saw
        // nothing change.
        "bg_color" => {
            if let Ok(c) = serde_json::from_value::<[f32; 4]>(value.clone()) {
                match &mut node.node_type {
                    GuiNodeType::Button { normal_color, .. } => *normal_color = c,
                    GuiNodeType::TextInput { bg_color, .. } => *bg_color = c,
                    _ => node.style.bg_color = c,
                }
            }
        }
        // A progress bar's fill: a timer bar that turns red as it runs out.
        // It used to be refused as unknown, so a
        // bar could change its length from a script but never its colour.
        "fg_color" => {
            if let Ok(c) = serde_json::from_value::<[f32; 4]>(value.clone()) {
                match &mut node.node_type {
                    GuiNodeType::ProgressBar { fg_color, .. } => *fg_color = c,
                    _ => return false,
                }
            }
        }
        // What a menu is animated with: fade it, pop it, round it.
        "opacity" => {
            if let Some(n) = value.as_f64() {
                node.style.opacity = (n as f32).clamp(0.0, 1.0);
            }
        }
        "scale" => {
            if let Some(n) = value.as_f64() {
                node.style.scale = (n as f32).max(0.0);
            }
        }
        // Sliding a toast in, nudging a card: the offset after layout.
        "offset_x" | "offset_y" => {
            if let Some(n) = value.as_f64() {
                if property == "offset_x" {
                    node.style.offset_x = n as f32;
                } else {
                    node.style.offset_y = n as f32;
                }
            }
        }
        "corner_radius" => {
            if let Some(n) = value.as_f64() {
                node.style.corner_radius = (n as f32).max(0.0);
            }
        }
        "border_color" => {
            if let Ok(c) = serde_json::from_value::<[f32; 4]>(value.clone()) {
                node.style.border_color = c;
            }
        }
        "border_width" => {
            if let Some(n) = value.as_f64() {
                node.style.border_width = (n as f32).max(0.0);
            }
        }
        "shadow" => {
            node.style.shadow = if value.is_null() { None } else { serde_json::from_value(value.clone()).ok() };
        }
        // Size and font size are settable so a script can fit a fixed-design
        // panel to the actual surface — the GUI system has no scale of its own.
        // A bare number is pixels. Only the full `{ "type": "Fixed", "value": n }`
        // form used to parse, so `gui_set(gui, "hp", "width", 120)` - how every
        // health bar in the arcade writes it - was dropped without a word, and
        // the bars never shrank.
        "width" | "height" => {
            let sv = serde_json::from_value::<SizeValue>(value.clone())
                .ok()
                .or_else(|| as_text(value).and_then(|s| s.trim().parse::<f32>().ok()).map(SizeValue::Fixed));
            if let Some(sv) = sv {
                if property == "width" {
                    node.style.width = sv;
                } else {
                    node.style.height = sv;
                }
            }
        }
        // Scrolling: the engine clamps the offset during layout, so a script
        // can set any number here and land at the nearest valid position.
        "scroll_x" | "scroll_y" => {
            if let Some(n) = as_text(value).and_then(|s| s.parse::<f32>().ok()) {
                if property == "scroll_x" {
                    node.style.scroll_x = n;
                } else {
                    node.style.scroll_y = n;
                }
            }
        }
        "overflow" => {
            if let Some(s) = as_text(value) {
                node.style.overflow = match s.as_str() {
                    "visible" | "Visible" => Overflow::Visible,
                    "hidden" | "Hidden" => Overflow::Hidden,
                    "scroll" | "Scroll" => Overflow::Scroll,
                    _ => return false,
                };
            }
        }
        "font_size" => {
            if let Some(n) = value.as_f64() {
                let n = n as f32;
                match &mut node.node_type {
                    GuiNodeType::Text { font_size, .. } => *font_size = n,
                    GuiNodeType::Button { font_size, .. } => *font_size = n,
                    _ => {}
                }
            }
        }
        _ => return false,
    }
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_document_default_roundtrip() {
        let doc = GuiDocument::default();
        let json = save_gui(&doc);
        let parsed = load_gui(&json).unwrap();
        assert_eq!(parsed.name, "Untitled GUI");
        assert_eq!(parsed.root.id, "root");
        assert!(parsed.root.children.is_empty());
    }

    #[test]
    fn gui_node_with_children_roundtrip() {
        let doc = GuiDocument {
            name: "HUD".to_string(),
            navigable: true,
            anchor: UIAnchor::TopLeft,
            root: GuiNode {
                id: "root".to_string(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    width: SizeValue::Percent(100.0),
                    height: SizeValue::Percent(100.0),
                    padding: EdgeInsets::all(8.0),
                    flex: FlexLayout {
                        direction: FlexDirection::Column,
                        gap: 4.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                children: vec![
                    GuiNode {
                        id: "score_label".to_string(),
                        node_type: GuiNodeType::Text {
                            text: "Score: 0".to_string(),
                            font_id: String::new(),
                            font_size: 24.0,
                            color: [1.0, 1.0, 1.0, 1.0],
                            alignment: GuiTextAlign::Left,
                            rich: false,
                        },
                        style: NodeStyle {
                            width: SizeValue::Auto,
                            height: SizeValue::Fixed(30.0),
                            ..Default::default()
                        },
                        children: Vec::new(),
                    },
                    GuiNode {
                        id: "health_bar".to_string(),
                        node_type: GuiNodeType::ProgressBar {
                            value: 0.75,
                            min: 0.0,
                            max: 1.0,
                            fg_color: [0.2, 0.7, 0.3, 1.0],
                            bg_color: [0.15, 0.15, 0.15, 1.0],
                            border_color: [0.0, 0.0, 0.0, 0.0],
                        },
                        style: NodeStyle {
                            width: SizeValue::Fixed(200.0),
                            height: SizeValue::Fixed(20.0),
                            ..Default::default()
                        },
                        children: Vec::new(),
                    },
                ],
            },
        };

        let json = save_gui(&doc);
        let parsed = load_gui(&json).unwrap();
        assert_eq!(parsed.name, "HUD");
        assert_eq!(parsed.root.children.len(), 2);

        let score = parsed.root.find_node("score_label").unwrap();
        if let GuiNodeType::Text { ref text, font_size, .. } = score.node_type {
            assert_eq!(text, "Score: 0");
            assert!((font_size - 24.0).abs() < f32::EPSILON);
        } else {
            panic!("Expected Text node");
        }

        let health = parsed.root.find_node("health_bar").unwrap();
        if let GuiNodeType::ProgressBar { value, .. } = health.node_type {
            assert!((value - 0.75).abs() < f32::EPSILON);
        } else {
            panic!("Expected ProgressBar node");
        }
    }

    #[test]
    fn all_widget_types_serialize() {
        let nodes = vec![
            GuiNode {
                id: "panel".to_string(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle::default(),
                children: Vec::new(),
            },
            GuiNode {
                id: "text".to_string(),
                node_type: GuiNodeType::Text {
                    text: "Hello".to_string(),
                    font_id: String::new(),
                    font_size: 16.0,
                    color: [1.0; 4],
                    alignment: GuiTextAlign::Center,
                    rich: false,
                },
                style: NodeStyle::default(),
                children: Vec::new(),
            },
            GuiNode {
                id: "image".to_string(),
                node_type: GuiNodeType::Image {
                    texture_id: "icon.png".to_string(),
                    tint: [1.0; 4],
                    uv_rect: None,
                },
                style: NodeStyle::default(),
                children: Vec::new(),
            },
            GuiNode {
                id: "button".to_string(),
                node_type: GuiNodeType::Button {
                    label: "Click Me".to_string(),
                    font_id: String::new(),
                    font_size: 16.0,
                    text_color: [1.0; 4],
                    normal_color: [0.3, 0.3, 0.3, 1.0],
                    hover_color: [0.4, 0.4, 0.4, 1.0],
                    pressed_color: [0.2, 0.2, 0.2, 1.0],
                    disabled: false,
                    alignment: Default::default(),
                    hover_scale: 1.0,
                },
                style: NodeStyle::default(),
                children: Vec::new(),
            },
            GuiNode {
                id: "progress".to_string(),
                node_type: GuiNodeType::ProgressBar {
                    value: 0.5,
                    min: 0.0,
                    max: 1.0,
                    fg_color: [0.2, 0.7, 0.3, 1.0],
                    bg_color: [0.15, 0.15, 0.15, 1.0],
                    border_color: [0.0; 4],
                },
                style: NodeStyle::default(),
                children: Vec::new(),
            },
            GuiNode {
                id: "slider".to_string(),
                node_type: GuiNodeType::Slider {
                    value: 0.5,
                    min: 0.0,
                    max: 1.0,
                    track_color: [0.3, 0.3, 0.3, 1.0],
                    thumb_color: [0.8, 0.8, 0.8, 1.0],
                    thumb_size: 16.0,
                },
                style: NodeStyle::default(),
                children: Vec::new(),
            },
            GuiNode {
                id: "toggle".to_string(),
                node_type: GuiNodeType::Toggle {
                    checked: true,
                    on_color: [0.2, 0.7, 0.3, 1.0],
                    off_color: [0.4, 0.4, 0.4, 1.0],
                    thumb_color: [1.0; 4],
                },
                style: NodeStyle::default(),
                children: Vec::new(),
            },
            GuiNode {
                id: "input".to_string(),
                node_type: GuiNodeType::TextInput {
                    text: String::new(),
                    placeholder: "Enter name…".to_string(),
                    font_id: String::new(),
                    font_size: 16.0,
                    color: [1.0; 4],
                    bg_color: [0.12, 0.12, 0.12, 1.0],
                    max_length: 0,
                    password: false,
                    edit: Default::default(),
                },
                style: NodeStyle::default(),
                children: Vec::new(),
            },
        ];

        for node in &nodes {
            let json = serde_json::to_string(node).unwrap();
            let parsed: GuiNode = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.id, node.id);
        }
    }

    #[test]
    fn find_node_deep() {
        let doc = GuiDocument {
            name: "Test".to_string(),
            navigable: true,
            anchor: UIAnchor::TopLeft,
            root: GuiNode {
                id: "root".to_string(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle::default(),
                children: vec![GuiNode {
                    id: "container".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle::default(),
                    children: vec![GuiNode {
                        id: "deep_label".to_string(),
                        node_type: GuiNodeType::Text {
                            text: "Found me!".to_string(),
                            font_id: String::new(),
                            font_size: 16.0,
                            color: [1.0; 4],
                            alignment: GuiTextAlign::Left,
                            rich: false,
                        },
                        style: NodeStyle::default(),
                        children: Vec::new(),
                    }],
                }],
            },
        };

        assert!(doc.root.find_node("deep_label").is_some());
        assert!(doc.root.find_node("nonexistent").is_none());
    }

    #[test]
    fn size_value_variants() {
        let fixed: SizeValue = serde_json::from_str(r#"{"type":"Fixed","value":100.0}"#).unwrap();
        assert_eq!(fixed, SizeValue::Fixed(100.0));

        let pct: SizeValue = serde_json::from_str(r#"{"type":"Percent","value":0.5}"#).unwrap();
        assert_eq!(pct, SizeValue::Percent(0.5));

        let auto: SizeValue = serde_json::from_str(r#"{"type":"Auto"}"#).unwrap();
        assert_eq!(auto, SizeValue::Auto);
    }

    #[test]
    fn collect_focusable_sorts_by_tab_order_then_document_order() {
        let root = GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle::default(),
            children: vec![
                GuiNode {
                    id: "btn_a".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        focusable: true,
                        tab_order: 2,
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
                GuiNode {
                    id: "btn_b".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        focusable: true,
                        tab_order: 1,
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
                GuiNode {
                    id: "btn_c".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        focusable: true,
                        tab_order: 1,
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
                GuiNode {
                    id: "not_focusable".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle::default(), // focusable defaults to false
                    children: Vec::new(),
                },
                GuiNode {
                    id: "hidden".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        focusable: true,
                        tab_order: 0,
                        visible: false,
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
            ],
        };

        let ids = root.collect_focusable();
        // btn_b (tab_order=1, doc=2) before btn_c (tab_order=1, doc=3) before btn_a (tab_order=2, doc=1)
        assert_eq!(ids, vec!["btn_b", "btn_c", "btn_a"]);
        // "not_focusable" excluded (focusable=false), "hidden" excluded (visible=false)
        assert!(!ids.contains(&"not_focusable".to_string()));
        assert!(!ids.contains(&"hidden".to_string()));
    }

    #[test]
    fn collect_focusable_empty_when_none() {
        let root = GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle::default(),
            children: vec![
                GuiNode {
                    id: "child".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle::default(),
                    children: Vec::new(),
                },
            ],
        };

        assert!(root.collect_focusable().is_empty());
    }

    #[test]
    fn collect_focusable_nested() {
        let root = GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle::default(),
            children: vec![
                GuiNode {
                    id: "outer".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        focusable: true,
                        tab_order: 2,
                        ..Default::default()
                    },
                    children: vec![
                        GuiNode {
                            id: "inner".to_string(),
                            node_type: GuiNodeType::Panel,
                            style: NodeStyle {
                                focusable: true,
                                tab_order: 0,
                                ..Default::default()
                            },
                            children: Vec::new(),
                        },
                    ],
                },
            ],
        };

        let ids = root.collect_focusable();
        // inner (tab_order=0) before outer (tab_order=2)
        assert_eq!(ids, vec!["inner", "outer"]);
    }

    #[test]
    fn apply_gui_node_prop_covers_every_documented_property() {
        // These are the properties docs/scripting-api.md promises gui_set
        // accepts. The dispatch used to be duplicated per runtime, so one
        // copy silently rejecting a name was invisible until something drew
        // wrong on screen. An unknown name returns false, so this catches a
        // property that gets dropped in a refactor.
        let mut img = GuiNode {
            id: "i".into(),
            node_type: GuiNodeType::Image {
                texture_id: "a.png".into(),
                tint: [1.0; 4],
                uv_rect: None,
            },
            style: NodeStyle::default(),
            children: vec![],
        };
        for (prop, val) in [
            ("texture_id", serde_json::json!("b.png")),
            ("uv_rect", serde_json::json!([0.0, 0.0, 0.5, 0.5])),
            ("tint", serde_json::json!([1.0, 0.0, 0.0, 1.0])),
            ("bg_color", serde_json::json!([0.1, 0.1, 0.1, 1.0])),
            ("visible", serde_json::json!(false)),
            ("width", serde_json::json!({ "type": "Fixed", "value": 12.0 })),
            ("height", serde_json::json!({ "type": "Fixed", "value": 8.0 })),
            ("constraint_left", serde_json::json!({ "type": "Px", "value": 4.0 })),
            ("constraint_top", serde_json::json!({ "type": "Px", "value": 6.0 })),
            ("focusable", serde_json::json!(true)),
            ("tab_order", serde_json::json!(3)),
            ("overflow", serde_json::json!("Scroll")),
            // Bridges flatten to strings at the FFI boundary and the native
            // translator re-parses them as JSON, so both shapes must land.
            ("scroll_x", serde_json::json!(12.5)),
            ("scroll_y", serde_json::json!("40")),
        ] {
            assert!(
                apply_gui_node_prop(&mut img, prop, &val),
                "gui_set property '{prop}' was rejected"
            );
        }

        // ...and the values actually landed, not just "was accepted".
        match &img.node_type {
            GuiNodeType::Image { texture_id, tint, uv_rect } => {
                assert_eq!(texture_id, "b.png");
                assert_eq!(*tint, [1.0, 0.0, 0.0, 1.0]);
                assert_eq!(*uv_rect, Some([0.0, 0.0, 0.5, 0.5]));
            }
            _ => panic!("node type changed"),
        }
        assert!(!img.style.visible);
        assert_eq!(img.style.width, SizeValue::Fixed(12.0));
        assert_eq!(img.style.constraints.left, Some(ConstraintValue::Px(4.0)));
        assert_eq!(img.style.tab_order, 3);
        assert_eq!(img.style.overflow, Overflow::Scroll);
        assert_eq!(img.style.scroll_x, 12.5);
        assert_eq!(img.style.scroll_y, 40.0);

        // An unparseable overflow name is a rejection, not a silent Visible.
        assert!(!apply_gui_node_prop(&mut img, "overflow", &serde_json::json!("sideways")));
        assert_eq!(img.style.overflow, Overflow::Scroll);

        assert!(!apply_gui_node_prop(&mut img, "no_such_property", &serde_json::json!(1)));

        // The button-only half of the documented list. `disabled` is the one
        // a shop needs: it dims the button and every runtime's hit-test
        // skips it.
        let mut btn = GuiNode {
            id: "b".into(),
            node_type: GuiNodeType::Button {
                label: "BUY".into(),
                font_id: String::new(),
                font_size: 16.0,
                text_color: [1.0; 4],
                normal_color: [0.2, 0.2, 0.2, 1.0],
                hover_color: [0.3, 0.3, 0.3, 1.0],
                pressed_color: [0.4, 0.4, 0.4, 1.0],
                disabled: false,
                alignment: Default::default(),
                hover_scale: 1.0,
            },
            style: NodeStyle::default(),
            children: vec![],
        };
        for (prop, val) in [
            ("disabled", serde_json::json!(true)),
            ("normal_color", serde_json::json!([0.1, 0.2, 0.3, 1.0])),
            ("hover_color", serde_json::json!([0.2, 0.3, 0.4, 1.0])),
            ("pressed_color", serde_json::json!([0.3, 0.4, 0.5, 1.0])),
        ] {
            assert!(
                apply_gui_node_prop(&mut btn, prop, &val),
                "gui_set property '{prop}' was rejected"
            );
        }
        match &btn.node_type {
            GuiNodeType::Button { disabled, normal_color, hover_color, pressed_color, .. } => {
                assert!(*disabled);
                assert_eq!(*normal_color, [0.1, 0.2, 0.3, 1.0]);
                assert_eq!(*hover_color, [0.2, 0.3, 0.4, 1.0]);
                assert_eq!(*pressed_color, [0.3, 0.4, 0.5, 1.0]);
            }
            _ => panic!("node type changed"),
        }
        assert!(btn.refuses_clicks());
        // `bg_color` on a Button is its face: the Button draws `normal_color`
        // and nothing reads its style's `bg_color`.
        assert!(apply_gui_node_prop(&mut btn, "bg_color", &serde_json::json!([0.9, 0.1, 0.1, 1.0])));
        match &btn.node_type {
            GuiNodeType::Button { normal_color, .. } => assert_eq!(*normal_color, [0.9, 0.1, 0.1, 1.0]),
            _ => panic!("node type changed"),
        }

        // A text input has two strings, and writing one must not write the
        // other: `text` is what the player typed and what `gui_get` reads,
        // `placeholder` is the grey prompt behind it.
        let mut input = GuiNode {
            id: "field".into(),
            node_type: GuiNodeType::TextInput {
                text: "typed".into(),
                placeholder: "prompt".into(),
                font_id: String::new(),
                font_size: 16.0,
                color: [1.0; 4],
                bg_color: [0.1, 0.1, 0.1, 1.0],
                max_length: 0,
                password: false,
                edit: Default::default(),
            },
            style: NodeStyle::default(),
            children: vec![],
        };
        assert!(apply_gui_node_prop(&mut input, "text", &serde_json::json!("")));
        assert!(apply_gui_node_prop(&mut input, "placeholder", &serde_json::json!("ask about...")));
        match &input.node_type {
            GuiNodeType::TextInput { text, placeholder, .. } => {
                assert_eq!(text, "", "the field was not cleared");
                assert_eq!(placeholder, "ask about...");
            }
            _ => panic!("node type changed"),
        }
    }

    #[test]
    fn gui_text_keeps_the_string_it_was_given() {
        assert_eq!(gui_prop_value("text", "28.00".into()), serde_json::json!("28.00"));
        assert_eq!(gui_prop_value("text", "007".into()), serde_json::json!("007"));
        assert_eq!(gui_prop_value("placeholder", "1".into()), serde_json::json!("1"));
        assert_eq!(gui_prop_value("value", "0.5".into()), serde_json::json!(0.5));
        assert_eq!(gui_prop_value("visible", "false".into()), serde_json::json!(false));
        assert_eq!(gui_prop_value("color", "[1,0,0,1]".into()), serde_json::json!([1, 0, 0, 1]));
    }

    #[test]
    fn apply_gui_node_prop_sets_text_and_font_size() {
        let mut t = GuiNode {
            id: "t".into(),
            node_type: GuiNodeType::Text {
                text: "old".into(),
                font_id: String::new(),
                font_size: 10.0,
                color: [1.0; 4],
                alignment: GuiTextAlign::default(),
                rich: false,
            },
            style: NodeStyle::default(),
            children: vec![],
        };
        assert!(apply_gui_node_prop(&mut t, "text", &serde_json::json!("new")));
        assert!(apply_gui_node_prop(&mut t, "font_size", &serde_json::json!(22.5)));
        assert!(apply_gui_node_prop(&mut t, "color", &serde_json::json!([0.0, 1.0, 0.0, 1.0])));
        match &t.node_type {
            GuiNodeType::Text { text, font_size, color, .. } => {
                assert_eq!(text, "new");
                assert_eq!(*font_size, 22.5);
                assert_eq!(*color, [0.0, 1.0, 0.0, 1.0]);
            }
            _ => panic!("node type changed"),
        }
    }

    #[test]
    fn apply_gui_node_prop_coerces_bridge_shaped_values() {
        // The bridges stringify then JSON-parse, so a number-shaped label
        // arrives as a Number and a "1" flag arrives as a Number too. Both
        // used to be silently dropped by the typed dispatch.
        let mut t = GuiNode {
            id: "t".into(),
            node_type: GuiNodeType::Text {
                text: "old".into(),
                font_id: String::new(),
                font_size: 10.0,
                color: [1.0; 4],
                alignment: GuiTextAlign::default(),
                rich: false,
            },
            style: NodeStyle::default(),
            children: vec![],
        };
        // gui_set(gui, "score_label", "text", "42") — straight from the docs.
        assert!(apply_gui_node_prop(&mut t, "text", &serde_json::json!(42)));
        match &t.node_type {
            GuiNodeType::Text { text, .. } => assert_eq!(text, "42"),
            _ => panic!("node type changed"),
        }
        assert!(apply_gui_node_prop(&mut t, "text", &serde_json::json!(true)));
        match &t.node_type {
            GuiNodeType::Text { text, .. } => assert_eq!(text, "true"),
            _ => panic!("node type changed"),
        }

        t.style.visible = true;
        assert!(apply_gui_node_prop(&mut t, "visible", &serde_json::json!(0)));
        assert!(!t.style.visible);
        assert!(apply_gui_node_prop(&mut t, "visible", &serde_json::json!("1")));
        assert!(t.style.visible);
        assert!(apply_gui_node_prop(&mut t, "visible", &serde_json::json!(false)));
        assert!(!t.style.visible);
    }

    #[test]
    fn a_progress_bars_fill_colour_can_be_set() {
        let mut bar = GuiNode {
            id: "b".into(),
            node_type: GuiNodeType::ProgressBar {
                value: 0.5,
                min: 0.0,
                max: 1.0,
                fg_color: [0.0, 1.0, 0.0, 1.0],
                bg_color: [0.0; 4],
                border_color: [0.0; 4],
            },
            style: NodeStyle::default(),
            children: vec![],
        };
        assert!(apply_gui_node_prop(&mut bar, "fg_color", &serde_json::json!([1.0, 0.2, 0.1, 1.0])));
        match &bar.node_type {
            GuiNodeType::ProgressBar { fg_color, .. } => assert_eq!(*fg_color, [1.0, 0.2, 0.1, 1.0]),
            _ => unreachable!(),
        }
        let mut text = GuiNode { id: "t".into(), node_type: GuiNodeType::Panel, style: NodeStyle::default(), children: vec![] };
        assert!(!apply_gui_node_prop(&mut text, "fg_color", &serde_json::json!([1.0, 0.0, 0.0, 1.0])), "only a bar has a fill");
    }

    #[test]
    fn a_bare_number_sets_a_width_in_pixels() {
        // Health bars shrink themselves this way - one game formats
        // "%.1f", others pass a number - and only
        // the `{ type, value }` form used to parse, so none of them moved.
        let mut bar = GuiNode {
            id: "bar".into(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle { width: SizeValue::Fixed(440.0), ..NodeStyle::default() },
            children: vec![],
        };
        for (value, want) in [
            (serde_json::json!(120), 120.0),
            (serde_json::json!(96.5), 96.5),
            (serde_json::json!("33.0"), 33.0),
            (serde_json::json!({ "type": "Fixed", "value": 12.0 }), 12.0),
        ] {
            assert!(apply_gui_node_prop(&mut bar, "width", &value));
            assert!(matches!(bar.style.width, SizeValue::Fixed(w) if w == want), "{value} gave {:?}", bar.style.width);
        }
        assert!(apply_gui_node_prop(&mut bar, "height", &serde_json::json!(7)));
        assert!(matches!(bar.style.height, SizeValue::Fixed(h) if h == 7.0));
        // Something that is neither leaves the size alone.
        assert!(apply_gui_node_prop(&mut bar, "width", &serde_json::json!("wide")));
        assert!(matches!(bar.style.width, SizeValue::Fixed(w) if w == 12.0));
    }

    #[test]
    fn parse_title_menu_gui_file() {
        let json = std::fs::read_to_string("tests/fixtures/title_menu.gui")
            .expect("title_menu.gui should exist");
        let doc = load_gui(&json).expect("title_menu.gui should parse");
        assert_eq!(doc.name, "Title Menu");
        assert_eq!(doc.root.children.len(), 4);
    }
}
