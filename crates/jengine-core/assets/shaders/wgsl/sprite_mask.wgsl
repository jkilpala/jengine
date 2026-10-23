
// === Sprite masks (renderer::mask) ===
// How much of a fragment at `world` this flush's mask keeps, 0..1. The mask
// is in `globals.mask_*`; kind 0 is no mask. `renderer::mask::coverage` is
// the same arithmetic on the CPU - change both together.

// A texture mask's texture (kind 4, `MaskShape::Texture`) - a white texel
// when the flush has none. Every sprite flush binds it (`WgpuSpriteBatch`).
@group(0) @binding(3) var mask_tex: texture_2d<f32>;
@group(0) @binding(4) var mask_samp: sampler;

fn mask_point(i: u32) -> vec2<f32> {
    let v = globals.mask_pts[i / 2u];
    return select(v.zw, v.xy, i % 2u == 0u);
}

fn mask_coverage(world: vec2<f32>) -> f32 {
    let kind = i32(globals.mask_info.x + 0.5);
    if (kind == 0) {
        return 1.0;
    }
    let l = vec2<f32>(
        dot(globals.mask_xf[0].xyz, vec3<f32>(world, 1.0)),
        dot(globals.mask_xf[1].xyz, vec3<f32>(world, 1.0)),
    );
    let h = max(globals.mask_pts[0].xy, vec2<f32>(1e-4));
    if (kind == 4) {
        // The texture's alpha over its quad, `mask_pts[1]` its uv rect (v
        // down, where world y is up). Sampled at level 0, so it needs no
        // derivatives and may run in any control flow.
        let q = l / h * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
        let r = globals.mask_pts[1];
        let a = textureSampleLevel(mask_tex, mask_samp, mix(r.xy, r.zw, q), 0.0).a;
        var keep = select(0.0, a, abs(l.x) <= h.x && abs(l.y) <= h.y);
        let cutoff = globals.mask_info.z;
        if (cutoff > 0.0) {
            keep = smoothstep(cutoff - 0.02, cutoff + 0.02, keep);
        }
        return select(keep, 1.0 - keep, globals.mask_info.y > 0.5);
    }
    var d = 0.0;
    if (kind == 1) {
        let q = abs(l) - h;
        d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0);
    } else if (kind == 2) {
        d = (length(l / h) - 1.0) * min(h.x, h.y);
    } else {
        let n = u32(globals.mask_info.z + 0.5);
        var inside = false;
        var dist = 1e30;
        var j = n - 1u;
        for (var i = 0u; i < n; i = i + 1u) {
            let a = mask_point(i);
            let b = mask_point(j);
            if ((a.y > l.y) != (b.y > l.y) && l.x < (b.x - a.x) * (l.y - a.y) / (b.y - a.y) + a.x) {
                inside = !inside;
            }
            let e = b - a;
            let t = clamp(dot(l - a, e) / max(dot(e, e), 1e-8), 0.0, 1.0);
            dist = min(dist, length(l - a - e * t));
            j = i;
        }
        d = select(dist, -dist, inside);
    }
    let feather = max(globals.mask_info.w, 1e-4);
    let keep = clamp(0.5 - d / feather, 0.0, 1.0);
    return select(keep, 1.0 - keep, globals.mask_info.y > 0.5);
}
