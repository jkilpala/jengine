// Phase 3.c-4 Step 6c — light composite WGSL.
//
// WGSL twin of `assets/shaders/light_composite.frag`. Multiplies the
// scene color by the accumulated light buffer so unlit areas darken
// toward the ambient color (which is what the light buffer is cleared
// to before lights are drawn additively into it).
//
// Shares the same fullscreen-triangle vertex stage technique as
// `wgsl/blit.wgsl` — generates positions from `vertex_index` so no VBO
// is needed. Bind group 0 has both source textures + a single shared
// linear sampler.

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       tex_coord:     vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    let x = f32((i32(vertex_index) & 1) << 2) - 1.0;
    let y = f32((i32(vertex_index) & 2) << 1) - 1.0;
    var out: VsOut;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.tex_coord     = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@group(0) @binding(0) var scene_tex:    texture_2d<f32>;
@group(0) @binding(1) var light_tex:    texture_2d<f32>;
@group(0) @binding(2) var lin_sampler:  sampler;

// x = the most light may be. 1.0 unless the scene is HDR: the light buffer
// was 8-bit, so light always stopped at 1.0 and every lit scene was tuned
// against that. See `WgpuLightingPass::set_hdr`.
// y = toon lighting's bands (0 is off), z = the softness of a band's edge -
// `ToonLighting`, stepped by `toon_step`.
// dither.x = the dither cell in world units (0 is none), and
// inv_view_projection takes a screen point to the world, where the dither
// pattern is anchored.
// The fog (`ecs::fog`): colour and density (w 0 is none); noise scale,
// noise amount, ground, height; drift x, drift y, scatter in pixels; and the
// box's centre and half size (half size 0 is everywhere).
struct Composite {
    max_light: vec4<f32>,
    dither: vec4<f32>,
    inv_view_projection: mat4x4<f32>,
    fog_color: vec4<f32>,
    fog_shape: vec4<f32>,
    fog_motion: vec4<f32>,
    fog_area: vec4<f32>,
};
@group(0) @binding(3) var<uniform> u: Composite;
// `Unlit` things, drawn in z order with the lit world into their own buffer
// (`WorldLayer::Emission`): added after the light, so they are at full
// brightness, and already cut out wherever something lit is in front.
@group(0) @binding(4) var emission_tex: texture_2d<f32>;
// The lights' highlights (`light_specular.wgsl`): added after the multiply,
// so a highlight is the light's colour whatever the surface's.
@group(0) @binding(5) var specular_tex: texture_2d<f32>;

// The 4x4 ordered-dither matrix, row by row (`lighting::BAYER4`).
const BAYER4 = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);

// The dither threshold, in (0, 1), of the world cell of `cell` units holding
// `p` (`lighting::bayer_threshold`).
fn bayer_threshold(p: vec2<f32>, cell: f32) -> f32 {
    let c = floor(p / cell);
    let ix = u32(c.x - 4.0 * floor(c.x / 4.0)) & 3u;
    let iy = u32(c.y - 4.0 * floor(c.y / 4.0)) & 3u;
    var m = BAYER4;
    return (m[iy * 4u + ix] + 0.5) / 16.0;
}

// Light stepped into bands by its brightest channel, so its colour is kept:
// the nearest band, its edge eased over `soft` of a band - or, with a dither
// cell, drawn as an ordered dither over that width.
// `renderer::lighting::toon_step` is the CPU's copy.
fn toon_step(light: vec3<f32>, bands: f32, soft: f32, dither: f32, p: vec2<f32>) -> vec3<f32> {
    let m = max(light.r, max(light.g, light.b));
    if (bands < 0.5 || m <= 1e-6) {
        return light;
    }
    let scaled = m * bands;
    let f = floor(scaled);
    let frac = scaled - f;
    let half = soft * 0.5;
    var up = select(0.0, 1.0, frac >= 0.5);
    if (dither > 0.0) {
        let threshold = 0.5 + (bayer_threshold(p, dither) - 0.5) * soft;
        up = select(0.0, 1.0, frac >= threshold);
    } else if (half > 1e-6) {
        up = smoothstep(0.5 - half, 0.5 + half, frac);
    }
    return light * ((f + up) / bands / m);
}

// A lattice cell to 0..1, with integers (`ecs::fog::hash`).
fn fog_hash(x: i32, y: i32) -> f32 {
    var h = u32(x) * 374761393u + u32(y) * 668265263u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return f32(h ^ (h >> 16u)) / 4294967295.0;
}

// Smooth value noise, 0..1 (`ecs::fog::value_noise`).
fn fog_noise(p: vec2<f32>) -> f32 {
    let c = floor(p);
    let f = p - c;
    let u = f * f * (vec2<f32>(3.0) - 2.0 * f);
    let x = i32(c.x);
    let y = i32(c.y);
    let a = fog_hash(x, y);
    let b = fog_hash(x + 1, y);
    let c2 = fog_hash(x, y + 1);
    let d = fog_hash(x + 1, y + 1);
    let top = a + (b - a) * u.x;
    let bottom = c2 + (d - c2) * u.x;
    return top + (bottom - top) * u.y;
}

// How thick the fog is at a world point (`ecs::fog::density_at`).
fn fog_density(p: vec2<f32>) -> f32 {
    var d = u.fog_color.w;
    if (u.fog_area.z > 0.0 && u.fog_area.w > 0.0) {
        let q = abs(p - u.fog_area.xy) - u.fog_area.zw;
        let soft = min(u.fog_area.z, u.fog_area.w) * 0.25;
        d = d * (1.0 - smoothstep(-soft, 0.0, max(q.x, q.y)));
    }
    if (u.fog_shape.w > 0.0) {
        let above = max(p.y - u.fog_shape.z, 0.0);
        d = d * exp(-above / u.fog_shape.w);
    }
    if (u.fog_shape.x > 0.0) {
        let q = (p - u.fog_motion.xy) / u.fog_shape.x;
        let n = 0.65 * fog_noise(q) + 0.35 * fog_noise(q * 2.03 + vec2<f32>(17.0));
        d = d * (1.0 + (n * 2.0 - 1.0) * u.fog_shape.y);
    }
    return clamp(d, 0.0, 1.0);
}

// The light the air round a pixel catches: the light buffer gathered over
// `scatter` pixels on two rings, so a lamp glows in the fog past its own
// reach and a shadow in the light is a shadow in the fog.
fn fog_scatter(uv: vec2<f32>, centre: vec3<f32>) -> vec3<f32> {
    let dims = vec2<f32>(textureDimensions(light_tex));
    let r = u.fog_motion.z / dims;
    var s = centre * 0.25;
    // The rings turned by a hash of the pixel: twelve fixed directions draw
    // a hard shadow's edge twelve times over, as steps; turned per pixel the
    // steps become a fine grain.
    let px = uv * dims;
    let turn = fog_hash(i32(px.x), i32(px.y)) * 0.5235988;
    for (var i = 0; i < 12; i = i + 1) {
        let a = f32(i) * 0.5235988 + turn;
        let k = select(0.45, 1.0, (i & 1) == 0);
        let o = vec2<f32>(cos(a), sin(a)) * r * k;
        s = s + textureSampleLevel(light_tex, lin_sampler, uv + o, 0.0).rgb * (0.75 / 12.0);
    }
    return clamp(s, vec3<f32>(0.0), vec3<f32>(u.max_light.x));
}

// Scene x light + emission: what both entries below write.
fn composite(in: VsOut) -> vec4<f32> {
    let scene = textureSample(scene_tex, lin_sampler, in.tex_coord);
    // Never below black: a subtractive light can take more than is there.
    let summed = clamp(textureSample(light_tex, lin_sampler, in.tex_coord).rgb, vec3<f32>(0.0), vec3<f32>(u.max_light.x));
    let ndc = vec2<f32>(in.tex_coord.x * 2.0 - 1.0, 1.0 - in.tex_coord.y * 2.0);
    let world = (u.inv_view_projection * vec4<f32>(ndc, 0.0, 1.0)).xy;
    let light = toon_step(summed, u.max_light.y, u.max_light.z, u.dither.x, world);
    let emission = textureSample(emission_tex, lin_sampler, in.tex_coord).rgb;
    let shine = max(textureSample(specular_tex, lin_sampler, in.tex_coord).rgb, vec3<f32>(0.0));
    var lit = scene.rgb * light + shine;
    // Fog: what is behind it veiled, the light it catches added.
    if (u.fog_color.w > 0.0) {
        let d = fog_density(world);
        let caught = fog_scatter(in.tex_coord, summed);
        lit = lit * (1.0 - d) + u.fog_color.rgb * caught * d;
    }
    return vec4<f32>(lit + emission, scene.a);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return composite(in);
}

// The lit colour of a layer whose scene buffer was cleared transparent, so
// premultiplied - the world above a Scene Color sprite, drawn over the frame
// by `WgpuLightingPass::composite_over` (premultiplied source-over).
@fragment
fn fs_over(in: VsOut) -> @location(0) vec4<f32> {
    let c = composite(in);
    return vec4<f32>(c.rgb, clamp(c.a, 0.0, 1.0));
}
