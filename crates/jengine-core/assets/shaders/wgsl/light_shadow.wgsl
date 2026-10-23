
// ---------------------------------------------------------------- shadows
//
// Appended to `point_light.wgsl` and `spot_light.wgsl` (each declares
// `u_light`, `u_occ` and `u_normals`), so the two lights cast one shadow.
// They used to carry a copy each. `renderer::lighting` transcribes all of
// it for `light_at`.

// Occluder segments shared by every light. `count.x` is the number of
// active segments; each `segs[i]` is `[x0, y0, x1, y1]` in world space
// and `seg_tint[i]` is `[r, g, b, opacity]` controlling how that edge
// shadows light (parallel to `segs`).
// Capacity must match OCCLUDER_POOL on the Rust side (renderer::lighting).
const OCCLUDER_POOL: u32 = 448u;
struct Occluders {
    count:     vec4<u32>,
    segs:      array<vec4<f32>, 448>,
    seg_tint:  array<vec4<f32>, 448>,
    // Each edge's owner, four to a vec4 (`LightData::occluder_owners`): the
    // occluder's index, and NO_SELF_SHADOW when it does not shadow its own
    // inside.
    seg_owner: array<vec4<u32>, 112>,
};

const NO_SELF_SHADOW: u32 = 0x80000000u;

fn owner_of(i: u32) -> u32 {
    return u_occ.seg_owner[i / 4u][i % 4u];
}

// `renderer::lighting::owner_bit`: five bits of the occluder's index.
fn owner_bit(o: u32) -> u32 {
    return 1u << (o & 31u);
}

// The near epsilon along a ray of `len`: a thousandth of it, or half a unit
// when that is less - `renderer::lighting::ray_eps_near`. A thousandth of a
// sun's 40,000-unit ray ignored every edge within forty units of a point.
fn ray_eps_near(len: f32) -> f32 {
    return min(0.001, 0.5 / max(len, 1e-6));
}

// Returns true if the open segment from `p` to `light` is crossed by
// the occluder segment `a`→`b` (i.e. the line of sight is blocked).
fn segment_blocks(p: vec2<f32>, light: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> bool {
    let r = light - p;
    let s = b - a;
    let denom = r.x * s.y - r.y * s.x;
    if (abs(denom) < 1e-6) {
        return false; // parallel — treat as no intersection
    }
    let qp = a - p;
    let t = (qp.x * s.y - qp.y * s.x) / denom; // along p -> light
    let u = (qp.x * r.y - qp.y * r.x) / denom; // along a -> b
    return t > ray_eps_near(length(r)) && t < 0.999 && u >= 0.0 && u <= 1.0;
}

// Number of samples across the light disc when computing soft shadows.
const SHADOW_SAMPLES: i32 = 8;

// RGB light transmitted through occluder `i` when its edge blocks the
// ray: `mix(white, tint, opacity)`. Opaque neutral occluders
// (`[0,0,0,1]`, the default) return black (full shadow); coloured or
// partially-opaque ones pass tinted/partial light.
fn occluder_transmit(i: u32) -> vec3<f32> {
    let t = u_occ.seg_tint[i];
    return mix(vec3<f32>(1.0), t.rgb, t.a);
}

// Does the ray `p` -> `light` cross the edge `a` -> `b`, for counting? An end
// on the ray's line counts as on its left, so a ray through a corner crosses
// once - `segment_blocks` counts both edges there, which is harmless for
// multiplying shadow and fatal for parity. `renderer::lighting::ray_crosses`.
fn ray_crosses(p: vec2<f32>, light: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> bool {
    let r = light - p;
    let side_a = r.x * (a.y - p.y) - r.y * (a.x - p.x) >= 0.0;
    let side_b = r.x * (b.y - p.y) - r.y * (b.x - p.x) >= 0.0;
    if (side_a == side_b) {
        return false;
    }
    let s = b - a;
    let denom = r.x * s.y - r.y * s.x;
    if (abs(denom) < 1e-12) {
        return false;
    }
    let qp = a - p;
    let t = (qp.x * s.y - qp.y * s.x) / denom;
    return t > ray_eps_near(length(r)) && t < 0.999;
}

// Where along the ray from `p` to `light` it crosses the edge `a`-`b`, as a
// distance from `p`; call only where `segment_blocks` said it does.
fn crossing_distance(p: vec2<f32>, light: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let r = light - p;
    let s = b - a;
    let denom = r.x * s.y - r.y * s.x;
    let qp = a - p;
    let t = (qp.x * s.y - qp.y * s.x) / denom;
    return t * length(r);
}

// How much of a bounded occluder's shadow falls on a point `near` units past
// it: 1 within its `shadow_length`, fading to 0 over the last quarter.
// `renderer::lighting::shadow_reach`.
fn shadow_reach(o: u32, near: f32) -> f32 {
    // `extra.z`: how much this light stretches bounded shadows (a sun's
    // `1 / height`; 1 for a lamp).
    let len = f32((o >> 20u) & 0x7FFu) * 4.0 * u_light.extra.z;
    return 1.0 - smoothstep(len * 0.75, len, near);
}

// Is the pixel under `frag` a caster (a sprite with an occluder)? The
// normal pass wrote its z under 0.475 (`fs_normal`).
fn is_caster(frag: vec4<f32>) -> bool {
    return textureLoad(u_normals, vec2<i32>(frag.xy), 0).z < 0.475;
}

// Owner flag: this occluder does not shadow casters.
const NOT_ON_CASTERS: u32 = 524288u; // 1 << 19

// The edges of this light's range that can matter to a pixel, one bit each
// (bit `j` of word `j / 32` is edge `first + j`): see `shadow_factor`.
const EDGE_WORDS: u32 = 14u; // OCCLUDER_POOL / 32
alias EdgeMask = array<u32, 14>;

// The index of `m`'s lowest set bit (`m` not zero): `m & -m` keeps only
// that bit, and five masks read its index off one binary digit at a time.
// `firstTrailingBit` says it in one call, but becomes GLSL's `findLSB`,
// which WebGL2's GLSL ES 3.00 does not have - the lights would not compile
// on the WebGL fallback. (A de Bruijn table lookup was a third slower.)
fn lowest_bit(m: u32) -> u32 {
    let x = m & (0u - m);
    return select(0u, 16u, (x & 0xFFFF0000u) != 0u)
        + select(0u, 8u, (x & 0xFF00FF00u) != 0u)
        + select(0u, 4u, (x & 0xF0F0F0F0u) != 0u)
        + select(0u, 2u, (x & 0xCCCCCCCCu) != 0u)
        + select(0u, 1u, (x & 0xAAAAAAAAu) != 0u);
}

// How far outside the wedge an edge may lie and still be tested. Only a
// false negative would be wrong, so this is generous: float error at a sun's
// 40,000 units is a few thousandths.
const WEDGE_MARGIN: f32 = 0.5;

// The triangle (`p`, `l0`, `l1`) a pixel's rays fill, ready to test edges
// against: its three sides' normals, and its extent along each (`lo`, `hi`,
// widened by `WEDGE_MARGIN`). Made once per pixel - only the edge's own axis
// is left to work out per edge.
struct Wedge {
    p: vec2<f32>,
    q0: vec2<f32>,
    q1: vec2<f32>,
    n0: vec2<f32>,
    n1: vec2<f32>,
    n2: vec2<f32>,
    lo: vec3<f32>,
    hi: vec3<f32>,
};

// A side's unit normal, or zero for a side of no length (a hard light's
// wedge is a line, and its far side is a point).
fn side_normal(d: vec2<f32>) -> vec2<f32> {
    let len = length(d);
    return select(vec2<f32>(-d.y, d.x) / len, vec2<f32>(0.0), len < 1e-6);
}

fn make_wedge(p: vec2<f32>, l0: vec2<f32>, l1: vec2<f32>) -> Wedge {
    // About `p`, so the numbers stay small near the pixel.
    var w: Wedge;
    w.p = p;
    w.q0 = l0 - p;
    w.q1 = l1 - p;
    w.n0 = side_normal(w.q0);
    w.n1 = side_normal(w.q1 - w.q0);
    w.n2 = side_normal(w.q1);
    // The triangle's corners are 0, q0 and q1; 0 projects to 0 on every axis.
    let a = vec3<f32>(dot(w.q0, w.n0), dot(w.q0, w.n1), dot(w.q0, w.n2));
    let b = vec3<f32>(dot(w.q1, w.n0), dot(w.q1, w.n1), dot(w.q1, w.n2));
    w.lo = min(vec3<f32>(0.0), min(a, b)) - vec3<f32>(WEDGE_MARGIN);
    w.hi = max(vec3<f32>(0.0), max(a, b)) + vec3<f32>(WEDGE_MARGIN);
    return w;
}

// Can the edge `a`-`b` touch any ray of the wedge? In 2D a triangle and a
// segment are apart exactly when a side of one separates them.
fn edge_meets_wedge(w: Wedge, a: vec2<f32>, b: vec2<f32>) -> bool {
    let qa = a - w.p;
    let qb = b - w.p;
    let ea = vec3<f32>(dot(qa, w.n0), dot(qa, w.n1), dot(qa, w.n2));
    let eb = vec3<f32>(dot(qb, w.n0), dot(qb, w.n1), dot(qb, w.n2));
    if (any(max(ea, eb) < w.lo) || any(min(ea, eb) > w.hi)) {
        return false;
    }
    // The edge's own side.
    let n = side_normal(qb - qa);
    if (all(n == vec2<f32>(0.0))) {
        return true;
    }
    let e = dot(qa, n);
    let t1 = dot(w.q0, n);
    let t2 = dot(w.q1, n);
    return !(e < min(0.0, min(t1, t2)) - WEDGE_MARGIN || e > max(0.0, max(t1, t2)) + WEDGE_MARGIN);
}

// Light reaching `frag` along one ray from `light`: every edge crossed
// multiplies in what it lets through - except the edges of an occluder that
// does not shadow itself and has `frag` inside it, and faded for one whose
// shadow has a length, measured from that occluder's nearest crossing.
// Inside is parity along this same ray, corrected by whether the light is
// inside (`u_light.occ.z`). `renderer::lighting::ray_transmit` is the CPU's
// copy, and says what the five-bit owner buckets can confuse.
//
// Only the edges in `cand` are visited; `shadow_factor` leaves out only
// edges no ray of its can cross, so the answer is the one every edge gives.
fn ray_transmit(frag: vec2<f32>, light: vec2<f32>, first: u32, words: u32, cand: ptr<function, EdgeMask>, on_caster: bool) -> vec3<f32> {
    var parity: u32 = 0u;
    var near: array<f32, 32>;
    if (u_light.occ.w != 0u) {
        for (var k: u32 = 0u; k < 32u; k = k + 1u) {
            near[k] = 3.4e38;
        }
        for (var w: u32 = 0u; w < words; w = w + 1u) {
            var m = (*cand)[w];
            loop {
                if (m == 0u) {
                    break;
                }
                let i = first + w * 32u + lowest_bit(m);
                m = m & (m - 1u);
                let o = owner_of(i);
                let seg = u_occ.segs[i];
                if ((o & NO_SELF_SHADOW) != 0u && ray_crosses(frag, light, seg.xy, seg.zw)) {
                    parity = parity ^ owner_bit(o);
                }
                if (((o >> 20u) & 0x7FFu) != 0u && segment_blocks(frag, light, seg.xy, seg.zw)) {
                    near[o & 31u] = min(near[o & 31u], crossing_distance(frag, light, seg.xy, seg.zw));
                }
            }
        }
    }
    let inside = parity ^ u_light.occ.z;
    var transmit = vec3<f32>(1.0);
    for (var w: u32 = 0u; w < words; w = w + 1u) {
        var m = (*cand)[w];
        loop {
            if (m == 0u) {
                break;
            }
            let i = first + w * 32u + lowest_bit(m);
            m = m & (m - 1u);
            let seg = u_occ.segs[i];
            if (segment_blocks(frag, light, seg.xy, seg.zw)) {
                let o = owner_of(i);
                if ((o & NO_SELF_SHADOW) != 0u && (inside & owner_bit(o)) != 0u) {
                    continue;
                }
                if (on_caster && (o & NOT_ON_CASTERS) != 0u) {
                    continue;
                }
                var reach = 1.0;
                if (((o >> 20u) & 0x7FFu) != 0u) {
                    reach = shadow_reach(o, near[o & 31u]);
                }
                transmit = transmit * mix(vec3<f32>(1.0), occluder_transmit(i), reach);
            }
        }
    }
    return transmit;
}

// Shadow transmittance per channel in [0, 1]: vec3(1) = fully lit,
// vec3(0) = fully shadowed. `light_size` is the apparent emitter
// radius — 0 yields a single hard ray, larger values sample the light
// as a disc perpendicular to the fragment→light direction, producing a
// penumbra. Light passing through multiple occluders multiplies their
// transmittances, so coloured/translucent occluders tint the shadow.
//
// Every ray leaves `frag` for a point between `l0` and `l1`, so an edge
// outside that wedge blocks none of them, is crossed by none (parity) and is
// nearest on none (shadow length). The wedge is tested once per edge, and the
// rays visit only what passed: they used to test every edge in reach, twice
// each, so a soft sun over a whole scene cost 8 x 2 x its edges per pixel -
// the showcase's Clay, 212 edges, spent ~90 ms a frame on it.
fn shadow_factor(frag: vec2<f32>, center: vec2<f32>, light_size: f32, on_caster: bool) -> vec3<f32> {
    // This light's own edges: `first .. first + n` of the pool.
    let first = u_light.occ.x;
    let n = min(u_light.occ.y, OCCLUDER_POOL - min(first, OCCLUDER_POOL));
    if (n == 0u) {
        return vec3<f32>(1.0);
    }
    let to_light = center - frag;
    let dist = length(to_light);
    if (dist < 1e-4) {
        return vec3<f32>(1.0);
    }
    let soft = light_size >= 0.001;
    let perp = vec2<f32>(-to_light.y, to_light.x) / dist;
    let reach = select(0.0, light_size, soft);
    let l0 = center - perp * reach;
    let l1 = center + perp * reach;

    let wedge = make_wedge(frag, l0, l1);
    var cand: EdgeMask;
    var found = false;
    for (var j: u32 = 0u; j < n; j = j + 1u) {
        let seg = u_occ.segs[first + j];
        if (edge_meets_wedge(wedge, seg.xy, seg.zw)) {
            cand[j / 32u] = cand[j / 32u] | (1u << (j % 32u));
            found = true;
        }
    }
    if (!found) {
        return vec3<f32>(1.0);
    }
    let words = (n + 31u) / 32u;

    // Hard path: single ray to the light center.
    if (!soft) {
        return ray_transmit(frag, center, first, words, &cand, on_caster);
    }

    // Soft path: sample the emitter disc across its perpendicular axis.
    var acc = vec3<f32>(0.0);
    for (var s: i32 = 0; s < SHADOW_SAMPLES; s = s + 1) {
        let off = (f32(s) / f32(SHADOW_SAMPLES - 1) - 0.5) * 2.0; // -1..1
        let sample_center = center + perp * (off * light_size);
        acc = acc + ray_transmit(frag, sample_center, first, words, &cand, on_caster);
    }
    return acc / f32(SHADOW_SAMPLES);
}
