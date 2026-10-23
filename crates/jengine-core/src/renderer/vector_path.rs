//! A [`VectorPath`] as triangles - what the sprite pass draws it with.
//!
//! **Why here, and pure.** Vector paths used to be tessellated inside their
//! own render pass, `render_shapes_wgpu`, which every runtime ran *after*
//! every sprite. So a path drew over every sprite whatever its `z_order` -
//! recorded in `docs/parity-gaps.md` as a deliberate difference, and exactly
//! wrong for a game that draws a route *under* the plane flying it. Since Sep
//! 2026 a path is one more kind of item in the interleaved sprite pass, sorted
//! with sprites, meshes, shadows and text by `z_order`, drawn with the sprite
//! pipeline against the white texture - the same move `Mesh2D` made.
//!
//! Tessellation is split out so it can be tested without a GPU: the part a
//! runtime could get wrong is which triangles, in which order, in what
//! colour, and none of that needs a device.

use crate::ecs::components::{PathCommand, VectorPath};

/// One solid-coloured piece of a path: its fill, or its stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct PathPart {
    /// In the path's own space; the entity's transform is applied at draw.
    pub vertices: Vec<[f32; 2]>,
    /// Triangles, three indices each.
    pub indices: Vec<u32>,
    pub color: [f32; 4],
}

/// The pieces to draw for `path`, **fill first, then stroke**, so an
/// outline sits on top of its own interior. Empty when there is nothing to
/// draw - no commands, no colours, or a shape lyon cannot tessellate.
pub fn tessellate(path: &VectorPath) -> Vec<PathPart> {
    use lyon::math::point;
    use lyon::path::Path;
    use lyon::tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions,
        StrokeTessellator, StrokeVertex, VertexBuffers,
    };

    if path.commands.is_empty() || (path.fill_color.is_none() && path.stroke_color.is_none()) {
        return Vec::new();
    }

    let mut builder = Path::builder();
    // lyon wants strict begin/end pairing per sub-path, so track whether one
    // is open: back-to-back MoveTo, explicit Close and the `closed` flag must
    // never end or close a sub-path that is not there.
    let mut open = false;
    for cmd in &path.commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                if open {
                    builder.end(false);
                }
                builder.begin(point(*x, *y));
                open = true;
            }
            PathCommand::LineTo { x, y } => {
                builder.line_to(point(*x, *y));
            }
            PathCommand::QuadTo { cx, cy, x, y } => {
                builder.quadratic_bezier_to(point(*cx, *cy), point(*x, *y));
            }
            PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y } => {
                builder.cubic_bezier_to(point(*c1x, *c1y), point(*c2x, *c2y), point(*x, *y));
            }
            PathCommand::Close => {
                if open {
                    builder.close();
                    open = false;
                }
            }
        }
    }
    if open {
        if path.closed {
            builder.close();
        } else {
            builder.end(false);
        }
    }
    let built = builder.build();

    let mut parts = Vec::new();
    if let Some(color) = path.fill_color {
        let mut geometry: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
        let ok = FillTessellator::new()
            .tessellate_path(
                &built,
                &FillOptions::default(),
                &mut BuffersBuilder::new(&mut geometry, |v: FillVertex| v.position().to_array()),
            )
            .is_ok();
        if ok && !geometry.indices.is_empty() {
            parts.push(PathPart { vertices: geometry.vertices, indices: geometry.indices, color });
        }
    }
    if let Some(color) = path.stroke_color {
        let mut geometry: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
        let ok = StrokeTessellator::new()
            .tessellate_path(
                &built,
                &StrokeOptions::default().with_line_width(path.stroke_width),
                &mut BuffersBuilder::new(&mut geometry, |v: StrokeVertex| v.position().to_array()),
            )
            .is_ok();
        if ok && !geometry.indices.is_empty() {
            parts.push(PathPart { vertices: geometry.vertices, indices: geometry.indices, color });
        }
    }
    parts
}

/// How far the farthest vertex of any part lies from the path's origin - for
/// culling a path the frame cannot see.
pub fn bounding_radius(parts: &[PathPart]) -> f32 {
    parts
        .iter()
        .flat_map(|p| p.vertices.iter())
        .map(|v| (v[0] * v[0] + v[1] * v[1]).sqrt())
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(fill: Option<[f32; 4]>, stroke: Option<[f32; 4]>) -> VectorPath {
        VectorPath {
            commands: vec![
                PathCommand::MoveTo { x: -10.0, y: -10.0 },
                PathCommand::LineTo { x: 10.0, y: -10.0 },
                PathCommand::LineTo { x: 10.0, y: 10.0 },
                PathCommand::LineTo { x: -10.0, y: 10.0 },
            ],
            fill_color: fill,
            stroke_color: stroke,
            stroke_width: 2.0,
            closed: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_filled_square_is_two_triangles_in_its_colour() {
        let parts = tessellate(&square(Some([1.0, 0.0, 0.0, 1.0]), None));
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].indices.len(), 6, "two triangles");
        assert_eq!(parts[0].color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn the_stroke_comes_after_the_fill_so_it_draws_on_top() {
        let parts = tessellate(&square(Some([1.0, 0.0, 0.0, 1.0]), Some([0.0, 0.0, 1.0, 1.0])));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].color, [1.0, 0.0, 0.0, 1.0], "fill first");
        assert_eq!(parts[1].color, [0.0, 0.0, 1.0, 1.0], "then its outline");
        // A stroke is a band either side of the edge: wider than the fill.
        assert!(bounding_radius(&parts[1..]) > bounding_radius(&parts[..1]));
    }

    /// An open polyline with only a stroke - a drawn route, which is the case
    /// the fold-in was made for.
    #[test]
    fn an_open_stroked_line_tessellates() {
        let route = VectorPath {
            commands: vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo { x: 100.0, y: 0.0 },
                PathCommand::LineTo { x: 100.0, y: 60.0 },
            ],
            // `VectorPath::default()` *fills* white - an open route left at the
            // default would be closed into a polygon and painted.
            fill_color: None,
            stroke_color: Some([1.0, 1.0, 1.0, 0.8]),
            stroke_width: 4.0,
            ..Default::default()
        };
        let parts = tessellate(&route);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].indices.len() >= 12, "two segments, a quad each at least");
        assert!(parts[0].indices.iter().all(|i| (*i as usize) < parts[0].vertices.len()));
    }

    #[test]
    fn nothing_to_draw_draws_nothing() {
        assert!(tessellate(&VectorPath::default()).is_empty(), "no commands");
        assert!(tessellate(&square(None, None)).is_empty(), "no colours");
    }
}
