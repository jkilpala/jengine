//! Layout engine — resolves a `GuiNode` tree into screen-space rectangles.
//!
//! The algorithm is a single-pass top-down traversal. Each node's final rect
//! is computed from its parent's content area, its own size rules
//! (`Fixed` / `Percent` / `Auto`), and the flex layout configuration.

use super::gui_document::*;

/// A computed screen-space rectangle for a single node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl LayoutRect {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    /// True when the rect encloses no pixels.
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// True when `(x, y)` falls inside the rect (edges inclusive).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    /// The overlapping region of two rects; empty (zero-sized) when disjoint.
    pub fn intersect(&self, other: &Self) -> Self {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.width).min(other.x + other.width);
        let y1 = (self.y + self.height).min(other.y + other.height);
        Self {
            x: x0,
            y: y0,
            width: (x1 - x0).max(0.0),
            height: (y1 - y0).max(0.0),
        }
    }
}

/// Convert a rect between the layout's top-left/y-down space and a y-up
/// screen space (or back — the transform is its own inverse).
///
/// Layout works in top-left coordinates. Renderers draw GUI quads through a
/// y-up GL ortho, so their rects have to be flipped. `set_scissor_rect`,
/// however, takes **framebuffer** coordinates, which are top-left/y-down like
/// the layout — so a clip that was flipped for the quads must be flipped back
/// before it becomes a scissor. Feeding the y-up value straight to the scissor
/// displaces it by `screen_h - 2y - height`, which looks like a container
/// clipping several dozen pixels too high: children escape past its top edge
/// while the last row is cut off at the bottom.
pub fn flip_y(rect: LayoutRect, screen_h: f32) -> LayoutRect {
    LayoutRect {
        y: screen_h - rect.y - rect.height,
        ..rect
    }
}

/// One node's resolved geometry.
///
/// `rect` is where the node wants to be; `clip` is the region it is actually
/// allowed to paint in, inherited from the nearest `Overflow::Hidden` /
/// `Overflow::Scroll` ancestor. Renderers scissor to `clip`, and hit-tests go
/// through [`LayoutEntry::contains`] so the hidden half of a half-scrolled
/// button is not clickable.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutEntry {
    pub id: String,
    pub rect: LayoutRect,
    /// Clip region inherited from a clipping ancestor, if any.
    pub clip: Option<LayoutRect>,
    /// `(max_scroll_x, max_scroll_y)` — set only on `Overflow::Scroll`
    /// containers, and zero on an axis whose content already fits.
    pub scroll_max: Option<(f32, f32)>,
    /// The node's scale with every ancestor's multiplied in: what its font
    /// sizes, border and corner radius are drawn at. `rect` is already
    /// scaled.
    pub scale: f32,
    /// The node's opacity with every ancestor's multiplied in.
    pub opacity: f32,
}

/// `p -> s * p + t`: an ancestor's scale about its centre, composed down
/// the tree.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Xf {
    s: f32,
    t: (f32, f32),
}

impl Xf {
    const IDENTITY: Xf = Xf { s: 1.0, t: (0.0, 0.0) };

    /// This transform, then a scale by `k` about `c` before it.
    fn then_scale_about(self, c: (f32, f32), k: f32) -> Xf {
        if k == 1.0 {
            return self;
        }
        Xf { s: self.s * k, t: (self.s * (1.0 - k) * c.0 + self.t.0, self.s * (1.0 - k) * c.1 + self.t.1) }
    }

    fn rect(self, r: LayoutRect) -> LayoutRect {
        if self == Xf::IDENTITY {
            return r;
        }
        LayoutRect {
            x: self.s * r.x + self.t.0,
            y: self.s * r.y + self.t.1,
            width: self.s * r.width,
            height: self.s * r.height,
        }
    }
}

/// What a node hands its children: the clip of the nearest clipping
/// ancestor, the ancestors' scale, and their opacity.
#[derive(Debug, Clone, Copy)]
struct Inherit {
    clip: Option<LayoutRect>,
    xf: Xf,
    opacity: f32,
}

impl LayoutEntry {
    /// The part of `rect` that survives clipping.
    pub fn visible_rect(&self) -> LayoutRect {
        match &self.clip {
            Some(c) => self.rect.intersect(c),
            None => self.rect,
        }
    }

    /// True when `(x, y)` hits the *visible* part of this node.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        if !self.rect.contains(x, y) {
            return false;
        }
        match &self.clip {
            Some(c) => c.contains(x, y),
            None => true,
        }
    }
}

/// Result of layout computation — a flat list of entries in tree-traversal
/// order (parent before children).
pub type LayoutResult = Vec<LayoutEntry>;

/// Pixels scrolled per unit of mouse-wheel delta.
///
/// One notch is `1.0` on every backend that feeds `InputState::mouse_wheel`,
/// so this is "one notch ≈ one 44px row plus its gap" for the button lists
/// this was built for.
pub const WHEEL_SCROLL_STEP: f32 = 58.0;

/// Compute layout for an entire GUI document.
///
/// Returns a flat vec of `(node_id, LayoutRect)` pairs in depth-first order.
/// Coordinates are in screen pixels with origin at top-left.
pub fn compute_layout(doc: &GuiDocument, screen_w: f32, screen_h: f32) -> LayoutResult {
    let mut result = Vec::new();

    // A hidden root hides the whole document. Children are skipped by their
    // parent's layout when hidden, but nothing looked at the root's own flag,
    // so `GUI.set(doc, "<root>", "visible", false)` hid the root's background
    // and left every child drawn and clickable. Drawing, hit-testing,
    // navigation and the accessibility tree all read this layout, so an empty
    // one is the single place they agree.
    if !doc.root.style.visible {
        return result;
    }

    // The root node occupies a rect derived from its size + the document anchor.
    let root_w = resolve_size(doc.root.style.width, screen_w);
    let root_h = resolve_size(doc.root.style.height, screen_h);

    let anchor = doc.anchor.origin(screen_w, screen_h);
    // UIAnchor::origin returns (x, y) with y=0 at bottom.
    // Convert to top-left origin: screen_y_top_left = screen_h - anchor_y.
    let anchor_x = anchor.x;
    let anchor_y = screen_h - anchor.y;

    // Offset the rect so that the anchor's alignment fraction of the rect
    // coincides with the anchor point (e.g. Center → rect center at screen center).
    let (fx, fy) = doc.anchor.alignment_fraction();
    let root_rect = LayoutRect {
        x: anchor_x - fx * root_w,
        y: anchor_y - fy * root_h,
        width: root_w,
        height: root_h,
    };

    layout_node(&doc.root, root_rect, Inherit { clip: None, xf: Xf::IDENTITY, opacity: 1.0 }, &mut result);
    result
}

/// Look up one node's rect in a computed layout.
pub fn rect_of(layout: &LayoutResult, node_id: &str) -> Option<LayoutRect> {
    layout.iter().find(|e| e.id == node_id).map(|e| e.rect)
}

/// Recursively compute layout for a node and its children.
///
/// `clip` is the region inherited from the nearest clipping ancestor - `None`
/// at the root, narrowed by every `Overflow::Hidden` / `Overflow::Scroll`
/// container on the way down.
fn layout_node(
    node: &GuiNode,
    rect: LayoutRect,
    inherit: Inherit,
    result: &mut LayoutResult,
) {
    let clip = inherit.clip;
    // Apply manual offset (for fine-tuning node position).
    let rect = LayoutRect {
        x: rect.x + node.style.offset_x,
        y: rect.y + node.style.offset_y,
        ..rect
    };

    // Content area = rect minus padding.
    let pad = &node.style.padding;
    let content_x = rect.x + pad.left;
    let content_y = rect.y + pad.top;
    let content_w = (rect.width - pad.horizontal()).max(0.0);
    let content_h = (rect.height - pad.vertical()).max(0.0);

    // A scrolling container shifts its children by a clamped offset, so the
    // scrollable range has to be known before anything is positioned.
    let (scroll_x, scroll_y, scroll_max) = if node.style.overflow.scrolls() {
        let (natural_w, natural_h) = measure_content(node, content_w, content_h);
        let max_x = (natural_w - content_w).max(0.0);
        let max_y = (natural_h - content_h).max(0.0);
        // The elastic stretch is added *after* the clamp: that is the whole
        // point of it being a separate field. `scroll_*` stays inside the
        // range so a script writing a huge value still means "the end", while
        // a pointer drag can pull the content past it and spring back.
        (
            node.style.scroll_x.clamp(0.0, max_x) + node.style.overscroll_x,
            node.style.scroll_y.clamp(0.0, max_y) + node.style.overscroll_y,
            Some((max_x, max_y)),
        )
    } else {
        (0.0, 0.0, None)
    };

    // Its own scale, about its own centre, on top of its ancestors'. The
    // layout below is done unscaled and every rect goes through `xf` on
    // the way out, so children scale with it.
    let xf = inherit.xf.then_scale_about((rect.x + rect.width * 0.5, rect.y + rect.height * 0.5), node.style.scale);
    let opacity = inherit.opacity * node.style.opacity.clamp(0.0, 1.0);
    result.push(LayoutEntry {
        id: node.id.clone(),
        rect: xf.rect(rect),
        clip,
        scroll_max,
        scale: xf.s,
        opacity,
    });

    if node.children.is_empty() {
        return;
    }

    // Children of a clipping container may only paint inside its content box.
    let child_clip = if node.style.overflow.clips() {
        let own = xf.rect(LayoutRect {
            x: content_x,
            y: content_y,
            width: content_w,
            height: content_h,
        });
        Some(match clip {
            Some(outer) => own.intersect(&outer),
            None => own,
        })
    } else {
        clip
    };

    let origin_x = content_x - scroll_x;
    let origin_y = content_y - scroll_y;
    let child_clip = Inherit { clip: child_clip, xf, opacity };

    // Dispatch to grid or flex layout (only for unconstrained children).
    if let Some(LayoutMode::Grid(ref grid)) = node.style.layout {
        layout_grid(
            node, grid, origin_x, origin_y, content_w, content_h, child_clip, result,
        );
    } else {
        layout_flex(
            node, origin_x, origin_y, content_w, content_h, child_clip, result,
        );
    }

    // Position constrained children absolutely within the content area.
    for child in &node.children {
        if !child.style.visible || child.style.constraints.is_empty() {
            continue;
        }
        let child_rect = resolve_constraints(child, origin_x, origin_y, content_w, content_h);
        layout_node(child, child_rect, child_clip, result);
    }
}

/// Natural (unclipped) size of a node's children, used to derive the
/// scrollable range of an `Overflow::Scroll` container.
///
/// Mirrors the sizing pass of [`layout_flex`] / [`layout_grid`] - it has to,
/// or the clamp would disagree with where the children actually land.
fn measure_content(node: &GuiNode, content_w: f32, content_h: f32) -> (f32, f32) {
    let flow: Vec<&GuiNode> = node
        .children
        .iter()
        .filter(|c| c.style.visible && c.style.constraints.is_empty())
        .collect();
    if flow.is_empty() {
        return (0.0, 0.0);
    }

    if let Some(LayoutMode::Grid(ref grid)) = node.style.layout {
        let num_cols = grid.columns.len().max(1);
        let needed_rows = flow.len().div_ceil(num_cols);
        let num_rows = grid.rows.len().max(needed_rows).max(1);
        let col_gap_total = grid.column_gap * (num_cols as f32 - 1.0).max(0.0);
        let row_gap_total = grid.row_gap * (num_rows as f32 - 1.0).max(0.0);
        let cols = resolve_tracks(&grid.columns, content_w - col_gap_total, num_cols);
        let rows = resolve_tracks(&grid.rows, content_h - row_gap_total, num_rows);
        return (
            cols.iter().sum::<f32>() + col_gap_total,
            rows.iter().sum::<f32>() + row_gap_total,
        );
    }

    let flex = &node.style.flex;
    let is_row = flex.direction == FlexDirection::Row;
    let gap_total = flex.gap * (flow.len() as f32 - 1.0).max(0.0);

    let mut total_main = 0.0f32;
    let mut max_cross = 0.0f32;
    for child in flow {
        let margin = &child.style.margin;
        let outer_w = resolve_size_or_auto(child, child.style.width, content_w, is_row, true)
            + margin.horizontal();
        let outer_h = resolve_size_or_auto(child, child.style.height, content_h, is_row, false)
            + margin.vertical();
        if is_row {
            total_main += outer_w;
            max_cross = max_cross.max(outer_h);
        } else {
            total_main += outer_h;
            max_cross = max_cross.max(outer_w);
        }
    }

    if is_row {
        (total_main + gap_total, max_cross)
    } else {
        (max_cross, total_main + gap_total)
    }
}

/// Apply a mouse-wheel notch to whichever `Overflow::Scroll` container sits
/// under the cursor, deepest first.
///
/// Lives here rather than in each runtime because the native runner, the
/// editor viewport and the web runtime all hit-test GUI documents themselves;
/// a copy per runtime is exactly how GUI features end up working in one of
/// them only. Returns `true` when a container's offset actually moved.
pub fn apply_gui_scroll(
    doc: &mut GuiDocument,
    mouse_x: f32,
    mouse_y: f32,
    wheel_delta: f32,
    screen_w: f32,
    screen_h: f32,
) -> bool {
    if wheel_delta == 0.0 || screen_w <= 0.0 || screen_h <= 0.0 {
        return false;
    }
    let layout = compute_layout(doc, screen_w, screen_h);
    // Deepest match wins: entries are emitted parent-before-child, so the last
    // hit is the innermost container under the cursor.
    let Some(target) = layout
        .iter()
        .rev()
        .find(|e| e.scroll_max.is_some() && e.contains(mouse_x, mouse_y))
    else {
        return false;
    };
    let (max_x, max_y) = target.scroll_max.unwrap_or((0.0, 0.0));
    let target_id = target.id.clone();
    let Some(node) = doc.root.find_node_mut(&target_id) else {
        return false;
    };
    // Wheel-up (positive delta) scrolls towards the top of the content.
    let step = -wheel_delta * WHEEL_SCROLL_STEP;
    let (before_x, before_y) = (node.style.scroll_x, node.style.scroll_y);
    if max_y > 0.0 {
        node.style.scroll_y = (before_y + step).clamp(0.0, max_y);
    } else if max_x > 0.0 {
        node.style.scroll_x = (before_x + step).clamp(0.0, max_x);
    }
    node.style.scroll_x != before_x || node.style.scroll_y != before_y
}

/// Resolve a child's rect from its edge-pinning constraints within a parent content area.
fn resolve_constraints(
    child: &GuiNode,
    parent_x: f32,
    parent_y: f32,
    parent_w: f32,
    parent_h: f32,
) -> LayoutRect {
    let c = &child.style.constraints;

    // Horizontal axis
    let (x, width) = match (c.left, c.right) {
        (Some(l), Some(r)) => {
            // Both edges pinned → stretch
            let lv = l.resolve(parent_w);
            let rv = r.resolve(parent_w);
            (parent_x + lv, (parent_w - lv - rv).max(0.0))
        }
        (Some(l), None) => {
            let lv = l.resolve(parent_w);
            let w = resolve_size_or_auto(child, child.style.width, parent_w, false, true);
            (parent_x + lv, w)
        }
        (None, Some(r)) => {
            let rv = r.resolve(parent_w);
            let w = resolve_size_or_auto(child, child.style.width, parent_w, false, true);
            (parent_x + parent_w - rv - w, w)
        }
        (None, None) => {
            // No horizontal constraint — center in parent (default for unconstrained axis)
            let w = resolve_size_or_auto(child, child.style.width, parent_w, false, true);
            (parent_x, w)
        }
    };

    // Vertical axis
    let (y, height) = match (c.top, c.bottom) {
        (Some(t), Some(b)) => {
            let tv = t.resolve(parent_h);
            let bv = b.resolve(parent_h);
            (parent_y + tv, (parent_h - tv - bv).max(0.0))
        }
        (Some(t), None) => {
            let tv = t.resolve(parent_h);
            let h = resolve_size_or_auto(child, child.style.height, parent_h, false, false);
            (parent_y + tv, h)
        }
        (None, Some(b)) => {
            let bv = b.resolve(parent_h);
            let h = resolve_size_or_auto(child, child.style.height, parent_h, false, false);
            (parent_y + parent_h - bv - h, h)
        }
        (None, None) => {
            let h = resolve_size_or_auto(child, child.style.height, parent_h, false, false);
            (parent_y, h)
        }
    };

    LayoutRect { x, y, width, height }
}

/// Lay out children using flex (row/column) rules.
#[allow(clippy::too_many_arguments)]
fn layout_flex(
    node: &GuiNode,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
    clip: Inherit,
    result: &mut LayoutResult,
) {
    let flex = &node.style.flex;
    let is_row = flex.direction == FlexDirection::Row;
    let gap = flex.gap;

    // First pass: measure each child's size (resolve Fixed/Percent, defer Auto).
    let mut child_sizes: Vec<(f32, f32)> = Vec::with_capacity(node.children.len());
    let mut total_main = 0.0f32;

    for child in &node.children {
        if !child.style.visible || !child.style.constraints.is_empty() {
            child_sizes.push((0.0, 0.0));
            continue;
        }
        let margin = &child.style.margin;
        let cw = resolve_size_or_auto(child, child.style.width, content_w, is_row, true);
        let ch = resolve_size_or_auto(child, child.style.height, content_h, is_row, false);
        let outer_w = cw + margin.horizontal();
        let outer_h = ch + margin.vertical();
        child_sizes.push((outer_w, outer_h));

        if is_row {
            total_main += outer_w;
        } else {
            total_main += outer_h;
        }
    }

    let visible_count = node
        .children
        .iter()
        .filter(|c| c.style.visible && c.style.constraints.is_empty())
        .count();
    let gap_total = if visible_count > 1 {
        gap * (visible_count as f32 - 1.0)
    } else {
        0.0
    };

    let main_extent = if is_row { content_w } else { content_h };
    let cross_extent = if is_row { content_h } else { content_w };
    let remaining = (main_extent - total_main - gap_total).max(0.0);

    // Compute starting offset and inter-child spacing based on justify_content.
    let (mut main_offset, extra_gap) = match flex.justify_content {
        JustifyContent::Start => (0.0, 0.0),
        JustifyContent::End => (remaining, 0.0),
        JustifyContent::Center => (remaining * 0.5, 0.0),
        JustifyContent::SpaceBetween => {
            if visible_count > 1 {
                (0.0, remaining / (visible_count as f32 - 1.0))
            } else {
                (0.0, 0.0)
            }
        }
        JustifyContent::SpaceAround => {
            if visible_count > 0 {
                let space = remaining / visible_count as f32;
                (space * 0.5, space)
            } else {
                (0.0, 0.0)
            }
        }
    };

    // Second pass: position each child.
    let mut first_visible = true;
    for (i, child) in node.children.iter().enumerate() {
        if !child.style.visible || !child.style.constraints.is_empty() {
            continue;
        }

        if !first_visible {
            main_offset += gap + extra_gap;
        }
        first_visible = false;

        let margin = &child.style.margin;
        let (outer_w, outer_h) = child_sizes[i];
        let inner_w = outer_w - margin.horizontal();
        let inner_h = outer_h - margin.vertical();

        let child_main = if is_row { outer_w } else { outer_h };
        let child_cross_outer = if is_row { outer_h } else { outer_w };
        let child_cross_inner = if is_row { inner_h } else { inner_w };

        // Cross-axis alignment
        let cross_offset = match flex.align_items {
            AlignItems::Start => 0.0,
            AlignItems::End => cross_extent - child_cross_outer,
            AlignItems::Center => (cross_extent - child_cross_outer) * 0.5,
            AlignItems::Stretch => 0.0,
        };

        let stretched_cross = if flex.align_items == AlignItems::Stretch {
            cross_extent
                - if is_row {
                    margin.vertical()
                } else {
                    margin.horizontal()
                }
        } else {
            child_cross_inner
        };

        let (child_x, child_y, child_w, child_h) = if is_row {
            (
                content_x + main_offset + margin.left,
                content_y + cross_offset + margin.top,
                inner_w,
                stretched_cross,
            )
        } else {
            (
                content_x + cross_offset + margin.left,
                content_y + main_offset + margin.top,
                stretched_cross,
                inner_h,
            )
        };

        let child_rect = LayoutRect {
            x: child_x,
            y: child_y,
            width: child_w.max(0.0),
            height: child_h.max(0.0),
        };

        layout_node(child, child_rect, clip, result);
        main_offset += child_main;
    }
}

/// Lay out children in a grid (rows × columns).
///
/// Tracks are resolved using CSS Grid-like rules:
/// - `Fixed(px)` — exact pixel size.
/// - `Fraction(fr)` — share of remaining space after fixed/auto tracks.
/// - `Auto` — sized to fit content (uses a minimum based on widget type).
///
/// Children are placed by their `grid_column`/`grid_row` style fields (0-based).
/// Children without explicit placement are auto-placed left-to-right, top-to-bottom.
#[allow(clippy::too_many_arguments)]
fn layout_grid(
    node: &GuiNode,
    grid: &GridLayout,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
    clip: Inherit,
    result: &mut LayoutResult,
) {
    let num_cols = grid.columns.len().max(1);
    let num_visible: usize = node.children.iter().filter(|c| c.style.visible && c.style.constraints.is_empty()).count();
    // Number of rows: max of explicit row definitions and the rows needed to fit all children.
    let needed_rows = (num_visible + num_cols - 1) / num_cols.max(1);
    let num_rows = grid.rows.len().max(needed_rows).max(1);

    // Resolve column widths.
    let col_gap_total = if num_cols > 1 {
        grid.column_gap * (num_cols as f32 - 1.0)
    } else {
        0.0
    };
    let col_widths = resolve_tracks(&grid.columns, content_w - col_gap_total, num_cols);

    // Resolve row heights.
    let row_gap_total = if num_rows > 1 {
        grid.row_gap * (num_rows as f32 - 1.0)
    } else {
        0.0
    };
    // Extend row definitions with Auto for any extra rows.
    let mut row_defs: Vec<GridTrack> = grid.rows.clone();
    while row_defs.len() < num_rows {
        row_defs.push(GridTrack::Auto);
    }
    let row_heights = resolve_tracks(&row_defs, content_h - row_gap_total, num_rows);

    // Compute cumulative offsets for columns and rows.
    let mut col_offsets = Vec::with_capacity(num_cols + 1);
    col_offsets.push(content_x);
    for (i, &w) in col_widths.iter().enumerate() {
        let prev = col_offsets[i];
        let gap = if i < num_cols - 1 { grid.column_gap } else { 0.0 };
        col_offsets.push(prev + w + gap);
    }

    let mut row_offsets = Vec::with_capacity(num_rows + 1);
    row_offsets.push(content_y);
    for (i, &h) in row_heights.iter().enumerate() {
        let prev = row_offsets[i];
        let gap = if i < num_rows - 1 { grid.row_gap } else { 0.0 };
        row_offsets.push(prev + h + gap);
    }

    // Place children into grid cells.
    let mut auto_cursor = 0usize; // linear index for auto-placement
    for child in &node.children {
        if !child.style.visible || !child.style.constraints.is_empty() {
            continue;
        }

        // Determine cell position.
        let (col, row) = if let (Some(c), Some(r)) = (child.style.grid_column, child.style.grid_row) {
            (c.min(num_cols - 1), r.min(num_rows - 1))
        } else {
            let c = auto_cursor % num_cols;
            let r = auto_cursor / num_cols;
            auto_cursor += 1;
            (c, r.min(num_rows - 1))
        };

        let cell_x = col_offsets[col];
        let cell_y = row_offsets[row];
        let cell_w = col_widths.get(col).copied().unwrap_or(0.0);
        let cell_h = row_heights.get(row).copied().unwrap_or(0.0);

        // Apply child margin within the cell.
        let margin = &child.style.margin;
        let child_x = cell_x + margin.left;
        let child_y = cell_y + margin.top;
        let child_w = (cell_w - margin.horizontal()).max(0.0);
        let child_h = (cell_h - margin.vertical()).max(0.0);

        let child_rect = LayoutRect {
            x: child_x,
            y: child_y,
            width: child_w,
            height: child_h,
        };

        layout_node(child, child_rect, clip, result);
    }
}

/// Resolve grid track sizes from definitions to pixel values.
fn resolve_tracks(defs: &[GridTrack], available: f32, count: usize) -> Vec<f32> {
    let mut sizes = vec![0.0f32; count];
    let mut remaining = available;
    let mut total_fr = 0.0f32;

    // First pass: fixed and auto tracks.
    for (i, def) in defs.iter().enumerate().take(count) {
        match def {
            GridTrack::Fixed(px) => {
                sizes[i] = *px;
                remaining -= px;
            }
            GridTrack::Auto => {
                // Auto tracks get a reasonable minimum; we don't have child sizes
                // available here, so use a heuristic.
                let auto_size = 40.0f32.min(available / count as f32);
                sizes[i] = auto_size;
                remaining -= auto_size;
            }
            GridTrack::Fraction(fr) => {
                total_fr += fr;
            }
        }
    }
    // Fill any tracks beyond the definitions with fraction(1).
    for size in sizes.iter_mut().skip(defs.len()) {
        total_fr += 1.0;
        *size = 0.0; // will be filled below
    }

    // Second pass: distribute remaining space to fraction tracks.
    remaining = remaining.max(0.0);
    if total_fr > 0.0 {
        for (i, def) in defs.iter().enumerate().take(count) {
            if let GridTrack::Fraction(fr) = def {
                sizes[i] = remaining * (fr / total_fr);
            }
        }
        // Extra tracks beyond definitions.
        for size in sizes.iter_mut().skip(defs.len()) {
            *size = remaining * (1.0 / total_fr);
        }
    }

    sizes
}

// ---------------------------------------------------------------------------
// Size resolution helpers
// ---------------------------------------------------------------------------

/// Resolve a `SizeValue` against a parent dimension. `Auto` falls back to parent.
fn resolve_size(size: SizeValue, parent: f32) -> f32 {
    match size {
        SizeValue::Fixed(v) => v,
        SizeValue::Percent(p) => parent * p / 100.0,
        SizeValue::Auto => parent,
    }
}

/// Resolve a `SizeValue` for a child, using heuristic for `Auto`:
/// - For the main axis: sum of children + gaps (or a sensible minimum).
/// - For the cross axis: max child (or parent if no children).
fn resolve_size_or_auto(
    node: &GuiNode,
    size: SizeValue,
    parent_extent: f32,
    is_row: bool,
    is_width: bool,
) -> f32 {
    match size {
        SizeValue::Fixed(v) => v,
        SizeValue::Percent(p) => parent_extent * p / 100.0,
        SizeValue::Auto => {
            // If this node has children, we could measure them. For now, use
            // a sensible minimum (content-dependent nodes like Text will use
            // their natural size). Fully recursive auto-sizing requires a
            // two-pass algorithm; we use a simplified approach here.
            let is_main_axis = (is_row && is_width) || (!is_row && !is_width);
            if node.children.is_empty() {
                // Leaf node: use a minimal size based on widget type.
                match &node.node_type {
                    GuiNodeType::Text { .. } => {
                        if is_width {
                            // Text width is hard to know without font metrics.
                            // Use parent extent as upper bound.
                            parent_extent
                        } else {
                            // Height: use a reasonable line height.
                            24.0
                        }
                    }
                    GuiNodeType::ProgressBar { .. } | GuiNodeType::Slider { .. } => {
                        if is_width {
                            200.0
                        } else {
                            20.0
                        }
                    }
                    GuiNodeType::Toggle { .. } => {
                        if is_width {
                            48.0
                        } else {
                            24.0
                        }
                    }
                    GuiNodeType::TextInput { .. } => {
                        if is_width {
                            200.0
                        } else {
                            32.0
                        }
                    }
                    GuiNodeType::Button { .. } => {
                        if is_width {
                            120.0
                        } else {
                            36.0
                        }
                    }
                    GuiNodeType::Image { .. } => {
                        // Without texture size info, pick a square default.
                        64.0
                    }
                    GuiNodeType::Circle { .. } => 48.0,
                    GuiNodeType::Panel => 0.0,
                }
            } else if is_main_axis {
                // Sum children along main axis (simplified: use parent extent).
                parent_extent
            } else {
                // Cross axis: use parent extent.
                parent_extent
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UIAnchor;

    /// A column of 6 fixed 40px rows (gap 10) inside a 100px-tall viewport:
    /// natural content is 6*40 + 5*10 = 290, so 190px of it can scroll.
    fn scroll_doc(scroll_y: f32) -> GuiDocument {
        let rows: Vec<GuiNode> = (0..6)
            .map(|i| GuiNode {
                id: format!("row{i}"),
                node_type: GuiNodeType::Button {
                    label: format!("row {i}"),
                    font_id: String::new(),
                    font_size: 16.0,
                    text_color: [1.0; 4],
                    normal_color: [0.2, 0.4, 0.8, 1.0],
                    hover_color: [0.3, 0.5, 0.9, 1.0],
                    pressed_color: [0.1, 0.3, 0.7, 1.0],
                    disabled: false,
                    alignment: Default::default(),
                    hover_scale: 1.0,
                },
                style: NodeStyle {
                    width: SizeValue::Fixed(80.0),
                    height: SizeValue::Fixed(40.0),
                    ..Default::default()
                },
                children: Vec::new(),
            })
            .collect();
        make_doc(GuiNode {
            id: "viewport".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(100.0),
                height: SizeValue::Fixed(100.0),
                overflow: Overflow::Scroll,
                scroll_y,
                flex: FlexLayout {
                    direction: FlexDirection::Column,
                    gap: 10.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            children: rows,
        })
    }

    #[test]
    fn flip_y_is_its_own_inverse() {
        // The renderer relies on this: quads are flipped into the y-up ortho,
        // and the clip that rode along with them is flipped back to become a
        // framebuffer-space scissor. If the two ever disagree, a scroll
        // container clips in the wrong place.
        let screen_h = 573.0;
        let r = LayoutRect { x: 12.0, y: 161.0, width: 476.0, height: 278.0 };
        let flipped = flip_y(r, screen_h);
        assert_eq!(flipped.y, screen_h - 161.0 - 278.0);
        assert_eq!(flip_y(flipped, screen_h), r);
        // A rect flush with the bottom of the layout space lands flush with
        // the top of the flipped space, and vice versa.
        let bottom = LayoutRect { x: 0.0, y: screen_h - 40.0, width: 10.0, height: 40.0 };
        assert_eq!(flip_y(bottom, screen_h).y, 0.0);
    }

    #[test]
    fn scroll_container_reports_its_range() {
        let result = compute_layout(&scroll_doc(0.0), 800.0, 600.0);
        let viewport = result.iter().find(|e| e.id == "viewport").unwrap();
        let (max_x, max_y) = viewport.scroll_max.unwrap();
        assert_eq!(max_x, 0.0, "content is narrower than the viewport");
        assert!(
            (max_y - 190.0).abs() < 0.01,
            "6 rows of 40 + 5 gaps of 10 = 290 in a 100px box leaves 190 to scroll, got {max_y}"
        );
    }

    #[test]
    fn scroll_offset_shifts_children_and_clamps() {
        let unscrolled = rect_of(&compute_layout(&scroll_doc(0.0), 800.0, 600.0), "row3").unwrap();
        let scrolled = rect_of(&compute_layout(&scroll_doc(50.0), 800.0, 600.0), "row3").unwrap();
        assert!(
            (unscrolled.y - scrolled.y - 50.0).abs() < 0.01,
            "scrolling 50px moves rows up by 50px"
        );

        // Past the end clamps to the range rather than running off.
        let clamped = rect_of(&compute_layout(&scroll_doc(9999.0), 800.0, 600.0), "row0").unwrap();
        let at_max = rect_of(&compute_layout(&scroll_doc(190.0), 800.0, 600.0), "row0").unwrap();
        assert!((clamped.y - at_max.y).abs() < 0.01, "scroll_y clamps to max");
        // Negative likewise clamps to the top.
        let negative = rect_of(&compute_layout(&scroll_doc(-100.0), 800.0, 600.0), "row0").unwrap();
        let top = rect_of(&compute_layout(&scroll_doc(0.0), 800.0, 600.0), "row0").unwrap();
        assert!((negative.y - top.y).abs() < 0.01, "scroll_y clamps to 0");
    }

    #[test]
    fn children_of_a_scroll_container_carry_its_clip() {
        let result = compute_layout(&scroll_doc(0.0), 800.0, 600.0);
        let viewport = result.iter().find(|e| e.id == "viewport").unwrap();
        assert!(viewport.clip.is_none(), "the container itself is unclipped");

        let row = result.iter().find(|e| e.id == "row0").unwrap();
        let clip = row.clip.expect("rows inherit the container's clip");
        assert_eq!(clip.height, 100.0);
        assert_eq!(clip.width, 100.0);

        // row0..row1 fit; the last row is entirely below the viewport.
        let last = result.iter().find(|e| e.id == "row5").unwrap();
        assert!(
            last.visible_rect().is_empty(),
            "a row scrolled out of view has nothing left to paint"
        );
    }

    #[test]
    fn hidden_rows_are_not_clickable() {
        let result = compute_layout(&scroll_doc(0.0), 800.0, 600.0);
        let row5 = result.iter().find(|e| e.id == "row5").unwrap();
        let (cx, cy) = (
            row5.rect.x + row5.rect.width * 0.5,
            row5.rect.y + row5.rect.height * 0.5,
        );
        assert!(row5.rect.contains(cx, cy), "the raw rect still contains it");
        assert!(
            !row5.contains(cx, cy),
            "but it is outside the clip, so a click there must miss"
        );

        let row0 = result.iter().find(|e| e.id == "row0").unwrap();
        assert!(row0.contains(
            row0.rect.x + row0.rect.width * 0.5,
            row0.rect.y + row0.rect.height * 0.5
        ));
    }

    #[test]
    fn overflow_hidden_clips_without_scrolling() {
        let mut doc = scroll_doc(0.0);
        doc.root.style.overflow = Overflow::Hidden;
        let result = compute_layout(&doc, 800.0, 600.0);
        let viewport = result.iter().find(|e| e.id == "viewport").unwrap();
        assert!(
            viewport.scroll_max.is_none(),
            "Hidden clips but has no scroll range"
        );
        assert!(result.iter().find(|e| e.id == "row0").unwrap().clip.is_some());
    }

    #[test]
    fn visible_overflow_is_unchanged_by_the_feature() {
        let mut doc = scroll_doc(0.0);
        doc.root.style.overflow = Overflow::Visible;
        doc.root.style.scroll_y = 40.0; // ignored without Overflow::Scroll
        let result = compute_layout(&doc, 800.0, 600.0);
        assert!(result.iter().all(|e| e.clip.is_none()));
        assert!(result.iter().all(|e| e.scroll_max.is_none()));
        let row0 = rect_of(&result, "row0").unwrap();
        assert_eq!(row0.y, rect_of(&compute_layout(&scroll_doc(0.0), 800.0, 600.0), "row0").unwrap().y);
    }

    #[test]
    fn wheel_scrolls_the_container_under_the_cursor() {
        let mut doc = scroll_doc(0.0);
        // The document is TopLeft-anchored at 100x100, so (50, 50) is inside.
        let moved = apply_gui_scroll(&mut doc, 50.0, 50.0, -1.0, 800.0, 600.0);
        assert!(moved, "a wheel notch inside the container scrolls it");
        assert!(
            (doc.root.style.scroll_y - WHEEL_SCROLL_STEP).abs() < 0.01,
            "one notch down = one step, got {}",
            doc.root.style.scroll_y
        );

        // Wheel up from the top has nowhere to go.
        doc.root.style.scroll_y = 0.0;
        assert!(!apply_gui_scroll(&mut doc, 50.0, 50.0, 1.0, 800.0, 600.0));

        // Outside the container, the wheel is not ours.
        assert!(!apply_gui_scroll(&mut doc, 400.0, 400.0, -1.0, 800.0, 600.0));
    }

    #[test]
    fn wheel_stops_at_the_bottom_of_the_content() {
        let mut doc = scroll_doc(0.0);
        for _ in 0..20 {
            apply_gui_scroll(&mut doc, 50.0, 50.0, -1.0, 800.0, 600.0);
        }
        assert!(
            (doc.root.style.scroll_y - 190.0).abs() < 0.01,
            "clamped to the scrollable range, got {}",
            doc.root.style.scroll_y
        );
    }

    fn make_doc(root: GuiNode) -> GuiDocument {
        GuiDocument {
            name: "Test".to_string(),
            navigable: true,
            anchor: UIAnchor::TopLeft,
            root,
        }
    }

    #[test]
    fn root_fills_screen() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Percent(100.0),
                height: SizeValue::Percent(100.0),
                ..Default::default()
            },
            children: Vec::new(),
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        assert_eq!(result.len(), 1);
        let entry = &result[0];
        let (id, rect) = (&entry.id, &entry.rect);
        assert_eq!(id, "root");
        assert!((rect.width - 800.0).abs() < f32::EPSILON);
        assert!((rect.height - 600.0).abs() < f32::EPSILON);
        assert!((rect.x).abs() < f32::EPSILON);
        assert!((rect.y).abs() < f32::EPSILON);
    }

    #[test]
    fn column_layout_with_fixed_children() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(200.0),
                height: SizeValue::Fixed(400.0),
                flex: FlexLayout {
                    direction: FlexDirection::Column,
                    gap: 10.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![
                GuiNode {
                    id: "a".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(100.0),
                        height: SizeValue::Fixed(50.0),
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
                GuiNode {
                    id: "b".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(100.0),
                        height: SizeValue::Fixed(50.0),
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        assert_eq!(result.len(), 3);

        let a = rect_of(&result, "a").unwrap();
        let b = rect_of(&result, "b").unwrap();

        // a starts at top of root
        assert!((a.y).abs() < f32::EPSILON);
        assert!((a.height - 50.0).abs() < f32::EPSILON);

        // b starts after a + gap
        assert!((b.y - 60.0).abs() < f32::EPSILON); // 50 + 10 gap
        assert!((b.height - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn row_layout_with_gap() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(100.0),
                flex: FlexLayout {
                    direction: FlexDirection::Row,
                    gap: 20.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![
                GuiNode {
                    id: "left".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(80.0),
                        height: SizeValue::Fixed(60.0),
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
                GuiNode {
                    id: "right".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(80.0),
                        height: SizeValue::Fixed(60.0),
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let left = rect_of(&result, "left").unwrap();
        let right = rect_of(&result, "right").unwrap();

        assert!((left.x).abs() < f32::EPSILON);
        assert!((left.width - 80.0).abs() < f32::EPSILON);
        assert!((right.x - 100.0).abs() < f32::EPSILON); // 80 + 20 gap
    }

    #[test]
    fn padding_reduces_content_area() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(200.0),
                height: SizeValue::Fixed(200.0),
                padding: EdgeInsets::all(10.0),
                ..Default::default()
            },
            children: vec![GuiNode {
                id: "child".to_string(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    width: SizeValue::Percent(100.0),
                    height: SizeValue::Percent(100.0),
                    ..Default::default()
                },
                children: Vec::new(),
            }],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let child = rect_of(&result, "child").unwrap();

        assert!((child.x - 10.0).abs() < f32::EPSILON);
        assert!((child.y - 10.0).abs() < f32::EPSILON);
        // content area = 200 - 20 padding = 180
        assert!((child.width - 180.0).abs() < f32::EPSILON);
        assert!((child.height - 180.0).abs() < f32::EPSILON);
    }

    #[test]
    fn justify_center() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(100.0),
                flex: FlexLayout {
                    direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![GuiNode {
                id: "centered".to_string(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    width: SizeValue::Fixed(100.0),
                    height: SizeValue::Fixed(50.0),
                    ..Default::default()
                },
                children: Vec::new(),
            }],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let centered = rect_of(&result, "centered").unwrap();
        // Remaining = 400 - 100 = 300, center offset = 150
        assert!((centered.x - 150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn align_items_center_cross_axis() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(200.0),
                flex: FlexLayout {
                    direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![GuiNode {
                id: "item".to_string(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    width: SizeValue::Fixed(80.0),
                    height: SizeValue::Fixed(40.0),
                    ..Default::default()
                },
                children: Vec::new(),
            }],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let item = rect_of(&result, "item").unwrap();
        // Cross axis (height): (200 - 40) / 2 = 80
        assert!((item.y - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn invisible_children_skipped() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(200.0),
                height: SizeValue::Fixed(200.0),
                flex: FlexLayout {
                    direction: FlexDirection::Column,
                    gap: 10.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![
                GuiNode {
                    id: "visible".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(100.0),
                        height: SizeValue::Fixed(50.0),
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
                GuiNode {
                    id: "hidden".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(100.0),
                        height: SizeValue::Fixed(50.0),
                        visible: false,
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
                GuiNode {
                    id: "after_hidden".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(100.0),
                        height: SizeValue::Fixed(50.0),
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let visible = rect_of(&result, "visible").unwrap();
        let after = rect_of(&result, "after_hidden").unwrap();

        // after_hidden should be at y = 50 + 10 gap = 60 (hidden child skipped)
        assert!((visible.y).abs() < f32::EPSILON);
        assert!((after.y - 60.0).abs() < f32::EPSILON);
    }

    /// `GUI.set(doc, "<root>", "visible", false)` hides the whole document:
    /// no entry is laid out (so nothing draws), no click lands on a child, and
    /// navigation finds nothing to focus. Before, only children's flags were
    /// read and the root's children stayed drawn and clickable (Crystal
    /// Clock, RPG Classics).
    #[test]
    fn a_hidden_root_hides_the_whole_document() {
        let child = |id: &str| GuiNode {
            id: id.to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(100.0),
                height: SizeValue::Fixed(50.0),
                focusable: true,
                ..Default::default()
            },
            children: Vec::new(),
        };
        let mut doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(200.0),
                height: SizeValue::Fixed(200.0),
                ..Default::default()
            },
            children: vec![child("a"), child("b")],
        });
        let mut assets = crate::asset::AssetManager::new();
        let visible: std::collections::HashSet<String> = ["menu.gui".to_string()].into_iter().collect();
        let click = |assets: &crate::asset::AssetManager| {
            click_hits([("menu.gui", "a")], &visible, assets, (10.0, 10.0), (800.0, 600.0))
        };

        assets.insert_gui("menu.gui".into(), doc.clone());
        assert_eq!(compute_layout(&doc, 800.0, 600.0).len(), 3, "shown: root and two children");
        assert_eq!(click(&assets).len(), 1, "shown: a click on a child lands");
        assert_eq!(doc.root.collect_focusable().len(), 2);

        // Hidden the way a script hides it: through the shared property setter.
        let root = doc.root.find_node_mut("root").unwrap();
        assert!(crate::ui::gui_document::apply_gui_node_prop(root, "visible", &serde_json::json!(false)));
        assets.insert_gui("menu.gui".into(), doc.clone());
        assert!(compute_layout(&doc, 800.0, 600.0).is_empty(), "hidden: nothing laid out, so nothing drawn");
        assert!(click(&assets).is_empty(), "hidden: no click lands on a child");
        assert!(doc.root.collect_focusable().is_empty(), "hidden: nothing to focus");

        // And shown again.
        let root = doc.root.find_node_mut("root").unwrap();
        assert!(crate::ui::gui_document::apply_gui_node_prop(root, "visible", &serde_json::json!(true)));
        assert_eq!(compute_layout(&doc, 800.0, 600.0).len(), 3);
    }

    #[test]
    fn space_between_justify() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(300.0),
                height: SizeValue::Fixed(100.0),
                flex: FlexLayout {
                    direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![
                GuiNode {
                    id: "first".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(50.0),
                        height: SizeValue::Fixed(50.0),
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
                GuiNode {
                    id: "last".to_string(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(50.0),
                        height: SizeValue::Fixed(50.0),
                        ..Default::default()
                    },
                    children: Vec::new(),
                },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let first = rect_of(&result, "first").unwrap();
        let last = rect_of(&result, "last").unwrap();

        assert!((first.x).abs() < f32::EPSILON);
        // remaining = 300 - 100 = 200, extra_gap = 200 / 1 = 200
        assert!((last.x - 250.0).abs() < f32::EPSILON); // 50 + 200
    }

    // =======================================================================
    // Grid layout tests
    // =======================================================================

    #[test]
    fn grid_2x2_equal_fractions() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(200.0),
                height: SizeValue::Fixed(200.0),
                layout: Some(LayoutMode::Grid(GridLayout {
                    columns: vec![GridTrack::Fraction(1.0), GridTrack::Fraction(1.0)],
                    rows: vec![GridTrack::Fraction(1.0), GridTrack::Fraction(1.0)],
                    column_gap: 0.0,
                    row_gap: 0.0,
                })),
                ..Default::default()
            },
            children: vec![
                GuiNode { id: "a".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
                GuiNode { id: "b".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
                GuiNode { id: "c".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
                GuiNode { id: "d".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let a = rect_of(&result, "a").unwrap();
        let b = rect_of(&result, "b").unwrap();
        let c = rect_of(&result, "c").unwrap();
        let d = rect_of(&result, "d").unwrap();

        // 200/2 = 100 per cell
        assert!((a.x).abs() < f32::EPSILON);
        assert!((a.y).abs() < f32::EPSILON);
        assert!((a.width - 100.0).abs() < f32::EPSILON);
        assert!((a.height - 100.0).abs() < f32::EPSILON);

        assert!((b.x - 100.0).abs() < f32::EPSILON);
        assert!((b.y).abs() < f32::EPSILON);

        assert!((c.x).abs() < f32::EPSILON);
        assert!((c.y - 100.0).abs() < f32::EPSILON);

        assert!((d.x - 100.0).abs() < f32::EPSILON);
        assert!((d.y - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn grid_with_gaps() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(210.0),
                height: SizeValue::Fixed(210.0),
                layout: Some(LayoutMode::Grid(GridLayout {
                    columns: vec![GridTrack::Fraction(1.0), GridTrack::Fraction(1.0)],
                    rows: vec![GridTrack::Fraction(1.0), GridTrack::Fraction(1.0)],
                    column_gap: 10.0,
                    row_gap: 10.0,
                })),
                ..Default::default()
            },
            children: vec![
                GuiNode { id: "a".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
                GuiNode { id: "b".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
                GuiNode { id: "c".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
                GuiNode { id: "d".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let a = rect_of(&result, "a").unwrap();
        let b = rect_of(&result, "b").unwrap();
        let c = rect_of(&result, "c").unwrap();

        // Available = 210 - 10 gap = 200, per column = 100
        assert!((a.width - 100.0).abs() < f32::EPSILON);
        assert!((a.height - 100.0).abs() < f32::EPSILON);

        // b starts at 100 + 10 gap = 110
        assert!((b.x - 110.0).abs() < f32::EPSILON);
        assert!((b.y).abs() < f32::EPSILON);

        // c starts at row 1: y = 100 + 10 gap = 110
        assert!((c.x).abs() < f32::EPSILON);
        assert!((c.y - 110.0).abs() < f32::EPSILON);
    }

    #[test]
    fn grid_fixed_and_fraction_columns() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(300.0),
                height: SizeValue::Fixed(100.0),
                layout: Some(LayoutMode::Grid(GridLayout {
                    columns: vec![GridTrack::Fixed(100.0), GridTrack::Fraction(1.0)],
                    rows: vec![GridTrack::Fraction(1.0)],
                    column_gap: 0.0,
                    row_gap: 0.0,
                })),
                ..Default::default()
            },
            children: vec![
                GuiNode { id: "fixed_col".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
                GuiNode { id: "flex_col".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let fixed = rect_of(&result, "fixed_col").unwrap();
        let flex = rect_of(&result, "flex_col").unwrap();

        assert!((fixed.width - 100.0).abs() < f32::EPSILON);
        // Remaining = 300 - 100 = 200, all goes to 1fr column
        assert!((flex.width - 200.0).abs() < f32::EPSILON);
        assert!((flex.x - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn grid_explicit_placement() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(200.0),
                height: SizeValue::Fixed(200.0),
                layout: Some(LayoutMode::Grid(GridLayout {
                    columns: vec![GridTrack::Fraction(1.0), GridTrack::Fraction(1.0)],
                    rows: vec![GridTrack::Fraction(1.0), GridTrack::Fraction(1.0)],
                    column_gap: 0.0,
                    row_gap: 0.0,
                })),
                ..Default::default()
            },
            children: vec![
                // Explicitly place in bottom-right cell (col=1, row=1)
                GuiNode {
                    id: "placed".into(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        grid_column: Some(1),
                        grid_row: Some(1),
                        ..Default::default()
                    },
                    children: vec![],
                },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let placed = rect_of(&result, "placed").unwrap();

        // Cell (1,1) = second column, second row => x=100, y=100
        assert!((placed.x - 100.0).abs() < f32::EPSILON);
        assert!((placed.y - 100.0).abs() < f32::EPSILON);
        assert!((placed.width - 100.0).abs() < f32::EPSILON);
        assert!((placed.height - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn grid_with_padding() {
        let doc = make_doc(GuiNode {
            id: "root".to_string(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(220.0),
                height: SizeValue::Fixed(220.0),
                padding: EdgeInsets::all(10.0),
                layout: Some(LayoutMode::Grid(GridLayout {
                    columns: vec![GridTrack::Fraction(1.0), GridTrack::Fraction(1.0)],
                    rows: vec![GridTrack::Fraction(1.0)],
                    column_gap: 0.0,
                    row_gap: 0.0,
                })),
                ..Default::default()
            },
            children: vec![
                GuiNode { id: "a".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
                GuiNode { id: "b".into(), node_type: GuiNodeType::Panel, style: Default::default(), children: vec![] },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let a = rect_of(&result, "a").unwrap();
        let b = rect_of(&result, "b").unwrap();

        // Content area = 220 - 20 padding = 200, per column = 100
        assert!((a.x - 10.0).abs() < f32::EPSILON);
        assert!((a.y - 10.0).abs() < f32::EPSILON);
        assert!((a.width - 100.0).abs() < f32::EPSILON);

        assert!((b.x - 110.0).abs() < f32::EPSILON);
    }

    // =======================================================================
    // Constraint layout tests
    // =======================================================================

    #[test]
    fn constraint_pin_left_top() {
        let doc = make_doc(GuiNode {
            id: "root".into(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(300.0),
                ..Default::default()
            },
            children: vec![GuiNode {
                id: "child".into(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    width: SizeValue::Fixed(80.0),
                    height: SizeValue::Fixed(60.0),
                    constraints: Constraints {
                        left: Some(ConstraintValue::Px(10.0)),
                        top: Some(ConstraintValue::Px(20.0)),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                children: vec![],
            }],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let child = rect_of(&result, "child").unwrap();
        assert!((child.x - 10.0).abs() < f32::EPSILON);
        assert!((child.y - 20.0).abs() < f32::EPSILON);
        assert!((child.width - 80.0).abs() < f32::EPSILON);
        assert!((child.height - 60.0).abs() < f32::EPSILON);
    }

    #[test]
    fn constraint_pin_left_right_stretches() {
        let doc = make_doc(GuiNode {
            id: "root".into(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(300.0),
                ..Default::default()
            },
            children: vec![GuiNode {
                id: "child".into(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    constraints: Constraints {
                        left: Some(ConstraintValue::Px(10.0)),
                        right: Some(ConstraintValue::Px(20.0)),
                        top: Some(ConstraintValue::Px(5.0)),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                children: vec![],
            }],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let child = rect_of(&result, "child").unwrap();
        assert!((child.x - 10.0).abs() < f32::EPSILON);
        assert!((child.width - 370.0).abs() < f32::EPSILON); // 400 - 10 - 20
        assert!((child.y - 5.0).abs() < f32::EPSILON);
    }

    #[test]
    fn constraint_pin_all_four_edges() {
        let doc = make_doc(GuiNode {
            id: "root".into(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(300.0),
                ..Default::default()
            },
            children: vec![GuiNode {
                id: "child".into(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    constraints: Constraints {
                        left: Some(ConstraintValue::Px(10.0)),
                        right: Some(ConstraintValue::Px(10.0)),
                        top: Some(ConstraintValue::Px(10.0)),
                        bottom: Some(ConstraintValue::Px(10.0)),
                    },
                    ..Default::default()
                },
                children: vec![],
            }],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let child = rect_of(&result, "child").unwrap();
        assert!((child.x - 10.0).abs() < f32::EPSILON);
        assert!((child.y - 10.0).abs() < f32::EPSILON);
        assert!((child.width - 380.0).abs() < f32::EPSILON); // 400 - 10 - 10
        assert!((child.height - 280.0).abs() < f32::EPSILON); // 300 - 10 - 10
    }

    #[test]
    fn constraint_percent_values() {
        let doc = make_doc(GuiNode {
            id: "root".into(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(200.0),
                ..Default::default()
            },
            children: vec![GuiNode {
                id: "child".into(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    constraints: Constraints {
                        left: Some(ConstraintValue::Percent(10.0)),  // 10% of 400 = 40
                        right: Some(ConstraintValue::Percent(10.0)), // 10% of 400 = 40
                        top: Some(ConstraintValue::Percent(25.0)),   // 25% of 200 = 50
                        ..Default::default()
                    },
                    ..Default::default()
                },
                children: vec![],
            }],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let child = rect_of(&result, "child").unwrap();
        assert!((child.x - 40.0).abs() < f32::EPSILON);
        assert!((child.width - 320.0).abs() < f32::EPSILON); // 400 - 40 - 40
        assert!((child.y - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn constraint_right_only_pins_to_right_edge() {
        let doc = make_doc(GuiNode {
            id: "root".into(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(300.0),
                ..Default::default()
            },
            children: vec![GuiNode {
                id: "child".into(),
                node_type: GuiNodeType::Panel,
                style: NodeStyle {
                    width: SizeValue::Fixed(100.0),
                    height: SizeValue::Fixed(50.0),
                    constraints: Constraints {
                        right: Some(ConstraintValue::Px(20.0)),
                        bottom: Some(ConstraintValue::Px(10.0)),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                children: vec![],
            }],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let child = rect_of(&result, "child").unwrap();
        assert!((child.x - 280.0).abs() < f32::EPSILON); // 400 - 20 - 100
        assert!((child.y - 240.0).abs() < f32::EPSILON); // 300 - 10 - 50
    }

    #[test]
    fn constrained_children_skip_flex() {
        // One child constrained, one in flex flow — they shouldn't affect each other's positioning.
        let doc = make_doc(GuiNode {
            id: "root".into(),
            node_type: GuiNodeType::Panel,
            style: NodeStyle {
                width: SizeValue::Fixed(400.0),
                height: SizeValue::Fixed(300.0),
                flex: FlexLayout {
                    direction: FlexDirection::Column,
                    gap: 10.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            children: vec![
                GuiNode {
                    id: "flex_child".into(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(100.0),
                        height: SizeValue::Fixed(50.0),
                        ..Default::default()
                    },
                    children: vec![],
                },
                GuiNode {
                    id: "constrained".into(),
                    node_type: GuiNodeType::Panel,
                    style: NodeStyle {
                        width: SizeValue::Fixed(80.0),
                        height: SizeValue::Fixed(40.0),
                        constraints: Constraints {
                            right: Some(ConstraintValue::Px(0.0)),
                            bottom: Some(ConstraintValue::Px(0.0)),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    children: vec![],
                },
            ],
        });

        let result = compute_layout(&doc, 800.0, 600.0);
        let flex_child = rect_of(&result, "flex_child").unwrap();
        let constrained = rect_of(&result, "constrained").unwrap();

        // flex_child at top of flex flow
        assert!((flex_child.x).abs() < f32::EPSILON);
        assert!((flex_child.y).abs() < f32::EPSILON);

        // constrained at bottom-right of parent
        assert!((constrained.x - 320.0).abs() < f32::EPSILON); // 400 - 0 - 80
        assert!((constrained.y - 260.0).abs() < f32::EPSILON); // 300 - 0 - 40
    }
}

/// The `gui_id`s some entity's `gui` component is showing - only those
/// documents can be clicked. Handlers are bound once in `init()` and kept for
/// the entity's life, so without this every button of every hidden panel
/// stayed clickable.
pub fn visible_gui_ids(world: &hecs::World) -> std::collections::HashSet<String> {
    world
        .query::<&crate::ecs::components::GuiComponent>()
        .iter()
        .filter(|(_, g)| g.visible)
        .map(|(_, g)| g.gui_id.clone())
        .collect()
}

/// Which of the bound `(gui_id, node_id)` pairs a click at `(mx, my)` hits -
/// in virtual pixels, on a UI laid out at `(sw, sh)`. A document that is not
/// `visible` or not loaded is skipped, a disabled node refuses
/// (`refuses_clicks`), and `contains` rejects the part of a row scrolled out
/// of its clipping container. Each document is laid out once, however many of
/// its nodes are bound.
///
/// One rule for every runtime: native (`ScriptServices::process_gui_clicks`)
/// and both browser hosts each hit-tested for themselves until Oct 2026, two
/// of them laying a document out again for every bound node.
pub fn click_hits<'a>(
    bindings: impl IntoIterator<Item = (&'a str, &'a str)>,
    visible: &std::collections::HashSet<String>,
    assets: &crate::asset::AssetManager,
    (mx, my): (f32, f32),
    (sw, sh): (f32, f32),
) -> Vec<(String, String)> {
    let mut layouts: std::collections::HashMap<&str, LayoutResult> = std::collections::HashMap::new();
    let mut hits = Vec::new();
    for (gui_id, node_id) in bindings {
        if !visible.contains(gui_id) {
            continue;
        }
        let Some(doc) = assets.get_gui(gui_id) else { continue };
        if doc.refuses_clicks(node_id) {
            continue;
        }
        let layout = layouts.entry(gui_id).or_insert_with(|| compute_layout(doc, sw, sh));
        if layout.iter().any(|e| e.id == node_id && e.contains(mx, my)) {
            hits.push((gui_id.to_string(), node_id.to_string()));
        }
    }
    hits
}
