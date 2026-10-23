// Phase 2 — first WGSL port of the existing `sprite.vert` + `sprite.frag` pair.
//
// Vertex layout matches `SpriteVertex` in `renderer/backend/wgpu/sprite.rs`:
//   location 0: vec2 position    (world-space, pre-transformed by the batcher)
//   location 1: vec3 uvq         (u * q, v * q, q): projective texture
//                                coordinates, divided by q per fragment. Every
//                                rectangle pushes q = 1; a textured mesh sets
//                                q per vertex for perspective-correct texturing.
//   location 2: vec4 color       (premultiplied tint)
//
// Bind group 0:
//   binding 0: globals.view_projection (mat4x4<f32>) + globals.time (f32)
//   binding 1: u_texture                (texture_2d<f32>)
//   binding 2: u_sampler                (sampler)
//
// This is the canonical shader the wgpu backend will use for all sprite draws
// once Phase 3 lands. WebGPU + WebGL2 (via naga's GLSL backend) both consume
// the same source.
//
// The `time` scalar lives in the same uniform buffer so graph-driven
// materials (built by `shader_codegen`) and the canonical sprite shader
// share a single bind-group layout (see Slice 13b).

struct Globals {
    view_projection: mat4x4<f32>,
    // x = seconds since runtime start; yzw reserved. Packed as a vec4 so
    // the uniform-address-space layout matches Rust's GlobalsUniform
    // exactly (a trailing scalar would require padding that naga rounds
    // up past our buffer size).
    time:            vec4<f32>,
    // The material parameters - read by graph shaders, declared here so the
    // mask after them is where the buffer has it.
    params:          array<vec4<f32>, 4>,
    // This flush's sprite mask (renderer::mask, sprite_mask.wgsl); kind 0 is none.
    mask_xf:         array<vec4<f32>, 2>,
    mask_info:       vec4<f32>,
    mask_pts:        array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var u_texture: texture_2d<f32>;
@group(0) @binding(2) var u_sampler: sampler;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uvq:      vec3<f32>,
    @location(2) color:    vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       uvq:           vec3<f32>,
    @location(1)       color:         vec4<f32>,
    // The vertex's world position, for the mask.
    @location(2)       world:         vec2<f32>,
};

// The vertex body, shared by `vs_main` (a vertex the CPU made) and
// `vs_instanced` (one an instance made, `sprite_instance.wgsl`).
fn sprite_vertex(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = globals.view_projection * vec4<f32>(in.position, 0.0, 1.0);
    out.uvq = in.uvq;
    out.color = in.color;
    out.world = in.position;
    return out;
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    return sprite_vertex(in);
}

fn sprite_color(in: VsOut) -> vec4<f32> {
    // Perspective divide. With q = 1 on every vertex this is (u, v) exactly.
    let uv = in.uvq.xy / in.uvq.z;
    let texel = textureSample(u_texture, u_sampler, uv);
    var c = texel * in.color;
    // A distance field (`globals.time.z`, the built-in font): the edge where
    // the distance crosses one half, as soft as one screen pixel - so text is
    // as sharp scaled up four times, or turned, as at its own size.
    let fw = max(fwidth(texel.a), 1e-4);
    if (globals.time.z > 0.5) {
        // A hair under one half: small text keeps the weight a coverage
        // bitmap had, which hinting and gamma made bolder than the outline.
        // `params[0]` moves the edge out (`.x`) and softens it (`.y`), in the
        // field's own units: a text outline or shadow (`TextOutline`,
        // `TextShadow`). Zero for plain text, so it draws as it always did.
        let edge = 0.47 - globals.params[0].x;
        let soft = fw * 0.85 + globals.params[0].y;
        // Never below zero: past the field's spread a glyph's rect reads 0,
        // and a band reaching under that would light the whole rect.
        let lo = max(edge - soft, 0.002);
        let hi = edge + soft;
        var a = smoothstep(lo, hi, texel.a);
        // Text drawn far smaller than the field (a 14 px HUD in a window
        // three quarters of the project's size) samples it five texels a
        // pixel apart, and one sample a pixel can step clean over a stem:
        // "sixes" drew as "s:xes". Past ~4 texels a pixel, average four
        // samples across the pixel. Under 3.5 nothing changes, so text at
        // its own size draws exactly as before. `small_text_keeps_its_thin_letters`.
        let dx = dpdx(uv);
        let dy = dpdy(uv);
        let size = vec2<f32>(textureDimensions(u_texture));
        let tpp = max(length(dx * size), length(dy * size));
        let blend = smoothstep(3.5, 4.5, tpp);
        if (blend > 0.0) {
            var sum = 0.0;
            for (var i = 0; i < 4; i = i + 1) {
                let o = vec2<f32>(select(-0.25, 0.25, (i & 1) == 1), select(-0.25, 0.25, i >= 2));
                let t = textureSampleLevel(u_texture, u_sampler, uv + dx * o.x + dy * o.y, 0.0).a;
                sum = sum + smoothstep(lo, hi, t);
            }
            a = mix(a, max(a, sum * 0.25), blend);
        }
        c = vec4<f32>(in.color.rgb, in.color.a * a);
    }
    // A threshold (`params[1].x`, a `canvas_group`'s): the picture's alpha
    // cut at that edge, its colour un-premultiplied - soft blobs drawn into
    // the group become one surface. The picture is premultiplied and drawn
    // so, hence the output is too. Zero for every other sprite.
    if (globals.params[1].x > 0.0) {
        let edge = globals.params[1].x;
        let soft = fw * 0.85 + globals.params[1].y;
        let a = smoothstep(max(edge - soft, 0.002), edge + soft, texel.a);
        let straight = texel.rgb / max(texel.a, 1e-4);
        c = vec4<f32>(straight * in.color.rgb * a, in.color.a * a);
    }
    // A silhouette (`params[2].x`, `ecs::silhouette`): the shape in one
    // colour (`params[3]`), outlined `params[2].y` texels wide - where the
    // texel is clear and a neighbour that far is not - and filled at
    // `params[2].z`. Zero for every other sprite.
    if (globals.params[2].x > 0.0 && globals.params[2].x < 1.5) {
        let o = globals.params[2].y / vec2<f32>(textureDimensions(u_texture));
        var near = 0.0;
        for (var k = 0; k < 8; k = k + 1) {
            let a = f32(k) * 0.78539816;
            let d = vec2<f32>(cos(a), sin(a)) * o;
            near = max(near, textureSampleLevel(u_texture, u_sampler, uv + d, 0.0).a);
        }
        let edge = clamp(near - texel.a, 0.0, 1.0);
        let shape = max(edge, texel.a * globals.params[2].z);
        c = vec4<f32>(globals.params[3].rgb, globals.params[3].a * shape * in.color.a);
    }
    // An outline (`params[2].x == 2`, `ecs::sprite_outline`): the sprite's
    // alpha grown outward - solid out to `params[1].y` world units, fading
    // to nothing over `params[1].z` past it - in `params[3]`. A world unit
    // is `params[2].yz` of texture coordinate; samples outside the sprite's
    // own rectangle (`params[0]`) read clear, so an atlas neighbour is not
    // ringed. Rings at the width and through the glow, sixteen directions
    // each, the glow's rings and directions turned a pixel at a time so its
    // fade dithers rather than banding into steps and spikes. Zero for every other sprite.
    if (globals.params[2].x > 1.5) {
        let lo = globals.params[0].xy;
        let hi = globals.params[0].zw;
        let width = globals.params[1].y;
        let glow = globals.params[1].z;
        let duv = globals.params[2].yz;
        let here = all(uv >= lo) && all(uv <= hi);
        var best = select(0.0, texel.a, here);
        let h = fract(sin(dot(floor(in.clip_position.xy), vec2<f32>(12.9898, 78.233))) * 43758.5453);
        for (var k = 0; k <= 6; k = k + 1) {
            // Ring 0 is the solid width; 1..6 walk out through the glow.
            var r = width;
            var fall = 1.0;
            if (k > 0) {
                r = width + glow * (f32(k) - h) / 6.0;
                fall = clamp(1.0 - (r - width) / max(glow, 1e-4), 0.0, 1.0);
            }
            if (fall <= best || r <= 0.0) {
                continue;
            }
            var near = 0.0;
            for (var j = 0; j < 16; j = j + 1) {
                let a = (f32(j) + h) * 0.39269908;
                let p = uv + vec2<f32>(cos(a), sin(a)) * r * duv;
                let inside = all(p >= lo) && all(p <= hi);
                near = max(near, select(0.0, textureSampleLevel(u_texture, u_sampler, p, 0.0).a, inside));
            }
            best = max(best, near * fall);
        }
        c = vec4<f32>(globals.params[3].rgb, globals.params[3].a * best * in.color.a);
    }
    // A masked draw keeps what its mask covers (`mask_coverage`, appended
    // from sprite_mask.wgsl).
    let keep = mask_coverage(in.world);
    if (keep <= 0.0) {
        discard;
    }
    return vec4<f32>(c.rgb, c.a * keep);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return sprite_color(in);
}

// For the multiply blends, which compute `src.rgb * dst + (1 - src.a) * dst`
// - a multiply only when `src.rgb` is premultiplied by its alpha. Straight,
// a transparent texel of a white texture came out `2 * dst`: every coloured
// multiply sprite brightened its whole quad wherever it was transparent.
@fragment
fn fs_premultiplied(in: VsOut) -> @location(0) vec4<f32> {
    let c = sprite_color(in);
    return vec4<f32>(c.rgb * c.a, c.a);
}
