// Ambient occlusion - the ambient light darkened near occluder outlines.
//
// Drawn once over the whole light buffer, right after it is cleared to the
// ambient colour and before any light is added, with a multiply blend: so it
// darkens the ambient term only, and a lamp still lights a corner.
//
// The rule is `renderer::lighting::ambient_occlusion_at`, transcribed: each
// occluder counts once, by its nearest edge - `1 - strength * (1 - d/r)^2`
// within `r` of it - and the counts multiply; a point inside an occluder (an
// odd number of its edges crossed by a ray from it, half-open at corners)
// takes nothing from that one. An occluder's edges are contiguous in the
// pool, and each carries its occluder's bounding box and where its edges end
// (`renderer::lighting::ao_occluder_bounds`): a pixel farther than the radius
// from the box takes nothing from it, and skips its edges after one test.

struct Ao {
    // Clip space back to world space: where each pixel is.
    inv_view_projection: mat4x4<f32>,
    // x = radius, y = strength, z = target width, w = target height (px)
    params: vec4<f32>,
};

// The same layout as the lights' occluder pool (`OccluderUniform`); here it
// holds the edges of every occluder near the view, whole - and, where the
// lights keep a tint and an owner, the occluder's box and its last edge.
struct Occluders {
    count:     vec4<u32>,
    segs:      array<vec4<f32>, 448>,
    // [min_x, min_y, max_x, max_y] of the edge's occluder.
    seg_box:   array<vec4<f32>, 448>,
    // One past the occluder's last edge, four to a vec4.
    seg_end:   array<vec4<u32>, 112>,
};

@group(0) @binding(0) var<uniform> u_ao: Ao;
@group(0) @binding(1) var<uniform> u_occ: Occluders;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    let x = f32((i32(vertex_index) & 1) << 2) - 1.0;
    let y = f32((i32(vertex_index) & 2) << 1) - 1.0;
    var out: VsOut;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}

fn group_end(i: u32) -> u32 {
    return u_occ.seg_end[i / 4u][i % 4u];
}

fn segment_distance(p: vec2<f32>, s: vec4<f32>) -> f32 {
    let a = s.xy;
    let ab = s.zw - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
    return distance(p, a + ab * t);
}

// `renderer::lighting::AO_RAY` and `ao_ray_crosses`: an end on the ray's line
// counts as on its left, so a ray through a corner crosses once; any crossing
// ahead of `p` counts.
const AO_RAY: vec2<f32> = vec2<f32>(1.0, 0.017);

fn ao_ray_crosses(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> bool {
    let d = AO_RAY;
    let side_a = d.x * (a.y - p.y) - d.y * (a.x - p.x) >= 0.0;
    let side_b = d.x * (b.y - p.y) - d.y * (b.x - p.x) >= 0.0;
    if (side_a == side_b) {
        return false;
    }
    let e = b - a;
    let denom = d.x * e.y - d.y * e.x;
    if (abs(denom) < 1e-12) {
        return false;
    }
    let qp = a - p;
    return (qp.x * e.y - qp.y * e.x) / denom > 0.0;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let size = u_ao.params.zw;
    let ndc = vec2<f32>(in.clip_position.x / size.x * 2.0 - 1.0, 1.0 - in.clip_position.y / size.y * 2.0);
    let w = u_ao.inv_view_projection * vec4<f32>(ndc, 0.0, 1.0);
    let p = w.xy / w.w;
    let radius = max(u_ao.params.x, 1e-3);
    let strength = clamp(u_ao.params.y, 0.0, 1.0);

    let n = u_occ.count.x;
    var total = 1.0;
    var i = 0u;
    loop {
        if (i >= n) {
            break;
        }
        let end = min(max(group_end(i), i + 1u), n);
        // Nearer to its box than the radius, or it cannot reach this pixel.
        let bb = u_occ.seg_box[i];
        let out = max(max(bb.xy - p, p - bb.zw), vec2<f32>(0.0));
        if (dot(out, out) >= radius * radius) {
            i = end;
            continue;
        }
        var nearest = 3.0e38;
        var crossings = 0u;
        loop {
            if (i >= end) {
                break;
            }
            let s = u_occ.segs[i];
            nearest = min(nearest, segment_distance(p, s));
            if (ao_ray_crosses(p, s.xy, s.zw)) {
                crossings = crossings + 1u;
            }
            i = i + 1u;
        }
        if ((crossings % 2u) == 0u && nearest < radius) {
            let k = 1.0 - nearest / radius;
            total = total * (1.0 - strength * k * k);
        }
    }
    return vec4<f32>(total, total, total, 1.0);
}
