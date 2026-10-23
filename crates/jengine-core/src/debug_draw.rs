//! Debug shapes: what scripts draw with `draw_debug_*`, and the physics
//! overlay (F4) that outlines every collider and joint - and, since Oct
//! 2026, where a `nav_region` lets an agent stand and where each walks.
//!
//! One shape type ([`DebugDrawCommand`]) and one renderer for all of it. The
//! web runtime used to own the only renderer, privately, so native collected
//! a script's `draw_debug_line` every frame and threw it away unseen; the
//! editor draws the same commands on its own canvas overlay.
//!
//! The physics overlay reads the ECS components rather than rapier, so it
//! shows what the scene *authored* - which is what you are checking when a
//! pendulum hangs from the wrong point. Sizes are not scaled by the
//! transform, matching how `PhysicsWorld::add_collider` builds them.

use hecs::World;

use crate::ecs::components::{
    BodyType, Collider2D, ColliderShape, Joint2D, JointType, Name, RigidBody2D, Transform,
};
pub use crate::scripting::api::DebugDrawCommand;

const STATIC: [f32; 3] = [0.6, 0.6, 0.68];
const DYNAMIC: [f32; 3] = [0.35, 1.0, 0.45];
const KINEMATIC: [f32; 3] = [0.3, 0.8, 1.0];
const SENSOR: [f32; 3] = [1.0, 0.9, 0.2];
const JOINT: [f32; 3] = [1.0, 0.55, 0.1];
/// A `nav_region`'s walkable outline and the grown shapes it walks round.
const NAV: [f32; 3] = [0.2, 0.85, 0.8];
/// A walking `nav_agent`'s path.
const NAV_PATH: [f32; 3] = [1.0, 0.4, 0.85];
/// Radius of the dot marking a joint's anchor on each body.
const ANCHOR_DOT: f32 = 4.0;

fn line(out: &mut Vec<DebugDrawCommand>, a: glam::Vec2, b: glam::Vec2, c: [f32; 3]) {
    out.push(DebugDrawCommand::Line { x1: a.x, y1: a.y, x2: b.x, y2: b.y, r: c[0], g: c[1], b: c[2] });
}

fn circle(out: &mut Vec<DebugDrawCommand>, at: glam::Vec2, radius: f32, c: [f32; 3]) {
    out.push(DebugDrawCommand::Circle { x: at.x, y: at.y, radius, r: c[0], g: c[1], b: c[2] });
}

fn polyline(out: &mut Vec<DebugDrawCommand>, pts: &[glam::Vec2], closed: bool, c: [f32; 3]) {
    for w in pts.windows(2) {
        line(out, w[0], w[1], c);
    }
    if closed && pts.len() > 2 {
        line(out, pts[pts.len() - 1], pts[0], c);
    }
}

/// Every collider's outline and every joint's link, in world space.
pub fn physics_debug_commands(world: &World) -> Vec<DebugDrawCommand> {
    let mut out = Vec::new();

    for (entity, (t, col)) in world.query::<(&Transform, &Collider2D)>().iter() {
        let color = if col.is_sensor {
            SENSOR
        } else {
            match world.get::<&RigidBody2D>(entity).map(|rb| rb.body_type) {
                Ok(BodyType::Dynamic) => DYNAMIC,
                Ok(BodyType::Kinematic) => KINEMATIC,
                _ => STATIC,
            }
        };
        let rot = glam::Mat2::from_angle(t.rotation);
        let local = |p: glam::Vec2| t.position + rot * (p + glam::Vec2::from(col.offset));
        let v = |p: &[f32; 2]| local(glam::Vec2::new(p[0], p[1]));
        match &col.shape {
            ColliderShape::Box { width, height } => {
                let (hw, hh) = (width / 2.0, height / 2.0);
                let pts = [
                    local(glam::Vec2::new(-hw, -hh)),
                    local(glam::Vec2::new(hw, -hh)),
                    local(glam::Vec2::new(hw, hh)),
                    local(glam::Vec2::new(-hw, hh)),
                ];
                polyline(&mut out, &pts, true, color);
            }
            ColliderShape::Circle { radius } => {
                let c = local(glam::Vec2::ZERO);
                circle(&mut out, c, *radius, color);
                // A radius line, so a rolling ball visibly rolls.
                line(&mut out, c, local(glam::Vec2::new(*radius, 0.0)), color);
            }
            ColliderShape::Capsule { half_height, radius } => {
                // `capsule_y`: two end circles joined by two straight sides.
                let (top, bottom) = (glam::Vec2::new(0.0, *half_height), glam::Vec2::new(0.0, -half_height));
                circle(&mut out, local(top), *radius, color);
                circle(&mut out, local(bottom), *radius, color);
                for sx in [-radius, *radius] {
                    line(&mut out, local(top + glam::Vec2::new(sx, 0.0)), local(bottom + glam::Vec2::new(sx, 0.0)), color);
                }
            }
            ColliderShape::Segment { a, b } => line(&mut out, v(a), v(b), color),
            ColliderShape::Triangle { a, b, c } => polyline(&mut out, &[v(a), v(b), v(c)], true, color),
            ColliderShape::ConvexPolygon { vertices } => {
                let pts: Vec<glam::Vec2> = vertices.iter().map(v).collect();
                polyline(&mut out, &pts, true, color);
            }
            ColliderShape::ConvexDecomposition { vertices, loops } => {
                for l in crate::ecs::components::decomposition_loops(vertices, loops) {
                    let pts: Vec<glam::Vec2> = l.iter().map(v).collect();
                    polyline(&mut out, &pts, true, color);
                }
            }
            ColliderShape::Polyline { vertices } => {
                let pts: Vec<glam::Vec2> = vertices.iter().map(v).collect();
                polyline(&mut out, &pts, false, color);
            }
            ColliderShape::HeightField { heights, scale } => {
                // rapier spreads the samples evenly across `scale.x`, centred.
                let n = heights.len();
                if n >= 2 {
                    let pts: Vec<glam::Vec2> = heights
                        .iter()
                        .enumerate()
                        .map(|(i, h)| {
                            let x = (i as f32 / (n - 1) as f32 - 0.5) * scale[0];
                            local(glam::Vec2::new(x, h * scale[1]))
                        })
                        .collect();
                    polyline(&mut out, &pts, false, color);
                }
            }
        }
    }

    // Joints: owner (body A, anchor A) to the named target (body B, anchor B),
    // which is how `ensure_physics_bodies` hands them to rapier.
    for (_, (t, joint)) in world.query::<(&Transform, &Joint2D)>().iter() {
        let Some(target) = world
            .query::<(&Name, &Transform)>()
            .iter()
            .find(|(_, (n, _))| n.0 == joint.target_entity)
            .map(|(_, (_, tt))| (tt.position, tt.rotation))
        else {
            continue;
        };
        let (anchor_a, anchor_b) = match &joint.joint_type {
            // These two used to report `[0, 0]` for their far end, which
            // was right only because the joint itself could not do
            // anything else. Now that it can, F4 has to draw where the
            // joint actually is or it would quietly disagree with the
            // simulation it exists to explain.
            JointType::Revolute { anchor, anchor_b, .. }
            | JointType::Prismatic { anchor, anchor_b, .. } => (*anchor, *anchor_b),
            JointType::Fixed { anchor_a, anchor_b }
            | JointType::Rope { anchor_a, anchor_b, .. }
            | JointType::Spring { anchor_a, anchor_b, .. }
            | JointType::Friction { anchor_a, anchor_b, .. } => (*anchor_a, *anchor_b),
            JointType::Wheel { anchor, anchor_b, .. } => (*anchor, *anchor_b),
            JointType::Motor { linear_offset, .. } => ([0.0, 0.0], *linear_offset),
            // Drawn to what it follows (a fixed point has no entity, and the
            // loop above skips it).
            JointType::Target { anchor, .. } => (*anchor, [0.0, 0.0]),
        };
        let a = t.position + glam::Mat2::from_angle(t.rotation) * glam::Vec2::from(anchor_a);
        let b = target.0 + glam::Mat2::from_angle(target.1) * glam::Vec2::from(anchor_b);
        line(&mut out, a, b, JOINT);
        circle(&mut out, a, ANCHOR_DOT, JOINT);
        circle(&mut out, b, ANCHOR_DOT, JOINT);
    }

    // Navigation: where an agent's centre may stand (the outline shrunk and
    // each obstacle grown by the agent radius), and each walker's way.
    for (_, region) in world.query::<&crate::navigation::NavRegion>().iter() {
        let Some(graph) = &region.baked else { continue };
        polyline(&mut out, graph.outline(), true, NAV);
        for pts in graph.obstacle_loops() {
            polyline(&mut out, pts, true, NAV);
        }
    }
    for (_, (t, agent)) in world.query::<(&Transform, &crate::navigation::NavAgent)>().iter() {
        let mut from = t.position;
        for p in &agent.path {
            let to = glam::Vec2::from(*p);
            line(&mut out, from, to, NAV_PATH);
            from = to;
        }
    }

    out
}

/// Thickness of a debug line in world units. Screen-space thickness would
/// need the camera zoom threaded in here; a fixed world width is what a
/// debug aid actually wants, and it scales with the view like everything
/// else on screen.
#[cfg(feature = "gpu-wgpu")]
const DEBUG_LINE_WIDTH: f32 = 2.0;

/// Draw debug shapes as quads through the sprite batch, flushed against the
/// `__white` texture (`flush(batch, None)`). No pipeline of its own.
#[cfg(feature = "gpu-wgpu")]
pub fn render_debug_commands(
    shapes: &[DebugDrawCommand],
    batch: &mut crate::renderer::backend::wgpu::WgpuSpriteBatch,
    flush: &mut impl FnMut(&mut crate::renderer::backend::wgpu::WgpuSpriteBatch, Option<&str>),
) {
    if shapes.is_empty() {
        return;
    }
    batch.begin();
    for shape in shapes {
        match *shape {
            DebugDrawCommand::Line { x1, y1, x2, y2, r, g, b } => {
                push_line(batch, (x1, y1), (x2, y2), [r, g, b]);
            }
            DebugDrawCommand::Rect { x, y, w, h, r, g, b } => {
                // Outline, not a fill: a filled rect would hide what it is
                // meant to annotate. `(x, y)` is the lower-left corner.
                let c = [r, g, b];
                push_line(batch, (x, y), (x + w, y), c);
                push_line(batch, (x + w, y), (x + w, y + h), c);
                push_line(batch, (x + w, y + h), (x, y + h), c);
                push_line(batch, (x, y + h), (x, y), c);
            }
            DebugDrawCommand::Circle { x, y, radius, r, g, b } => {
                // 24 segments reads as a circle at any size a debug shape is
                // useful at, without spending vertices on it.
                const SEGMENTS: usize = 24;
                let mut prev = (x + radius, y);
                for i in 1..=SEGMENTS {
                    let theta = (i as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
                    let next = (x + radius * theta.cos(), y + radius * theta.sin());
                    push_line(batch, prev, next, [r, g, b]);
                    prev = next;
                }
            }
        }
    }
    flush(batch, None);
}

#[cfg(feature = "gpu-wgpu")]
fn push_line(
    batch: &mut crate::renderer::backend::wgpu::WgpuSpriteBatch,
    from: (f32, f32),
    to: (f32, f32),
    color: [f32; 3],
) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f32::EPSILON {
        return;
    }
    // A line is a quad as long as the segment, as thick as the pen, rotated
    // to the segment's angle and centred on its midpoint.
    batch.draw_sprite_uv(
        glam::Vec2::new((from.0 + to.0) * 0.5, (from.1 + to.1) * 0.5),
        glam::Vec2::new(length, DEBUG_LINE_WIDTH),
        dy.atan2(dx),
        glam::Vec4::new(color[0], color[1], color[2], 1.0),
        [0.0, 0.0, 1.0, 1.0],
    );
}

/// The editor draws debug shapes on its own canvas overlay from the same JSON
/// a script's `draw_debug_*` produces; this is that JSON for one command.
pub fn debug_command_json(cmd: &DebugDrawCommand) -> String {
    match *cmd {
        DebugDrawCommand::Line { x1, y1, x2, y2, r, g, b } => serde_json::json!({
            "t": "ddl", "x1": x1, "y1": y1, "x2": x2, "y2": y2, "r": r, "g": g, "b": b
        }),
        DebugDrawCommand::Rect { x, y, w, h, r, g, b } => serde_json::json!({
            "t": "ddr", "x": x, "y": y, "w": w, "h": h, "r": r, "g": g, "b": b
        }),
        DebugDrawCommand::Circle { x, y, radius, r, g, b } => serde_json::json!({
            "t": "ddc", "x": x, "y": y, "rad": radius, "r": r, "g": g, "b": b
        }),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::RigidBody2D;

    fn collider(shape: ColliderShape) -> Collider2D {
        // Every other field has a serde default.
        serde_json::from_value(serde_json::json!({ "shape": serde_json::to_value(&shape).unwrap() }))
            .unwrap()
    }

    #[test]
    fn a_box_is_four_lines_around_its_transform() {
        let mut world = World::new();
        world.spawn((
            Transform { position: glam::Vec2::new(100.0, 50.0), rotation: 0.0, scale: glam::Vec2::ONE },
            collider(ColliderShape::Box { width: 20.0, height: 10.0 }),
        ));
        let cmds = physics_debug_commands(&world);
        assert_eq!(cmds.len(), 4);
        let xs: Vec<f32> = cmds
            .iter()
            .flat_map(|c| match c {
                DebugDrawCommand::Line { x1, x2, .. } => vec![*x1, *x2],
                _ => vec![],
            })
            .collect();
        assert!(xs.iter().all(|x| (*x - 90.0).abs() < 1e-3 || (*x - 110.0).abs() < 1e-3));
    }

    #[test]
    fn a_joint_links_the_owner_anchor_to_the_target() {
        let mut world = World::new();
        world.spawn((
            Name("Pivot".into()),
            Transform { position: glam::Vec2::new(0.0, 200.0), rotation: 0.0, scale: glam::Vec2::ONE },
        ));
        world.spawn((
            Transform { position: glam::Vec2::new(0.0, 120.0), rotation: 0.0, scale: glam::Vec2::ONE },
            RigidBody2D::default(),
            Joint2D {
                target_entity: "Pivot".into(),
                joint_type: JointType::Revolute {
                    anchor: [0.0, 80.0],
                    anchor_b: [0.0, 0.0],
                    limits: None,
                    motor_speed: 0.0,
                    motor_max_torque: 0.0,
                },
                collide_connected: false,
                ..Default::default()
            },
        ));
        let cmds = physics_debug_commands(&world);
        let Some(DebugDrawCommand::Line { x1, y1, x2, y2, .. }) = cmds.first() else {
            panic!("no joint line: {cmds:?}");
        };
        // Anchor 80 above the bob is the pivot's position.
        assert_eq!((*x1, *y1, *x2, *y2), (0.0, 200.0, 0.0, 200.0));
    }
}
