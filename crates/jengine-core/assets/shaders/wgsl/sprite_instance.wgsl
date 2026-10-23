
// One rectangle per instance - `WgpuSpriteBatch`'s instanced sprites (Oct
// 2026). A sprite used to be four vertices the CPU turned and wrote, 144
// bytes; it is one record of 68 now, and its corners are made here from
// `vertex_index` 0..3 (top-left, top-right, bottom-right, bottom-left - the
// order of the batch's index buffer's first quad). The arithmetic is the
// CPU's, step for step, so a sprite lands where it did.
//
// Appended to the default sprite shader and to every generated graph, which
// both define `VsIn`, `VsOut` and `sprite_vertex(in: VsIn) -> VsOut` - the
// vertex body `vs_main` runs - so an instanced sprite is the same vertex
// handed to the same body, graphs that move their vertices included.

struct InstIn {
    @location(3) center: vec2<f32>,
    // The rectangle in the sprite's own frame: left, bottom, right, top.
    @location(4) rect: vec4<f32>,
    // cos and sin of its turn, and a horizontal shear (shadows lean).
    @location(5) turn: vec3<f32>,
    // u_min, v_min, u_max, v_max.
    @location(6) uv: vec4<f32>,
    @location(7) tint: vec4<f32>,
};

fn instance_vertex(vi: u32, inst: InstIn) -> VsIn {
    let right = vi == 1u || vi == 2u;
    let top = vi == 0u || vi == 1u;
    let y = select(inst.rect.y, inst.rect.w, top);
    var x = select(inst.rect.x, inst.rect.z, right);
    if (inst.turn.z != 0.0) {
        x = x + inst.turn.z * y;
    }
    let rx = x * inst.turn.x - y * inst.turn.y;
    let ry = x * inst.turn.y + y * inst.turn.x;
    var v: VsIn;
    v.position = inst.center + vec2<f32>(rx, ry);
    v.uvq = vec3<f32>(select(inst.uv.x, inst.uv.z, right), select(inst.uv.w, inst.uv.y, top), 1.0);
    v.color = inst.tint;
    return v;
}

@vertex
fn vs_instanced(@builtin(vertex_index) vi: u32, inst: InstIn) -> VsOut {
    return sprite_vertex(instance_vertex(vi, inst));
}
