// Lens flares (Oct 2026): the reflections a bright light leaves inside a
// camera's lens, after John Chapman's "pseudo lens flare". Everything past
// `threshold` is taken down to a quarter of the frame and blurred, and from
// that soft copy each pixel looks back through the centre of the screen: so
// every light bright enough throws a chain of soft, coloured ghosts across
// the frame on the line through the centre, a halo, and - with `streak` - a
// long horizontal anamorphic streak. The fringes come from reading red,
// green and blue a little apart. Five passes, all but the last at a quarter
// of the frame:
//
//   fs_bright    the frame, past the threshold, at a quarter size
//   fs_blur_h/v  softened, so a ghost is a disc and a streak is a line
//   fs_features  ghosts, halo and streak, read from the soft copy
//   fs_composite the frame plus the features, tinted
//
// Uniforms (64 bytes):
//   threshold  - brightness below which nothing flares.
//   intensity  - how strongly the flare is added.
//   ghosts     - how many ghosts (0 - 8); spacing - how far apart.
//   halo       - the halo's distance, in screen heights (0 for none);
//   halo_width - its softness.
//   dispersion - how far apart red and blue are read (chromatic fringes).
//   streak     - the anamorphic streak's strength (0 for none).
//   streak_len - its half length, in screen widths.
//   aspect     - width / height.
//   texel      - one texel of the quarter-size copy, in UV.
//   tint       - the flare's colour.

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       tex_coord:     vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    let x = f32((i32(vertex_index) & 1) << 2) - 1.0;
    let y = f32((i32(vertex_index) & 2) << 1) - 1.0;
    var out: VsOut;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.tex_coord     = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

struct LensFlareUniforms {
    threshold:  f32,
    intensity:  f32,
    ghosts:     f32,
    spacing:    f32,
    halo:       f32,
    halo_width: f32,
    dispersion: f32,
    streak:     f32,
    streak_len: f32,
    aspect:     f32,
    texel:      vec2<f32>,
    tint:       vec4<f32>,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: LensFlareUniforms;
// fs_composite only: the features, at a quarter size.
@group(0) @binding(3) var flare_texture: texture_2d<f32>;

fn read(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src_texture, src_sampler, uv, 0.0).rgb;
}

// The frame past the threshold, four bilinear taps covering the 4 x 4
// texels under each quarter-size texel. Capped, so one white-hot pixel
// cannot flare the whole frame.
@fragment
fn fs_bright(in: VsOut) -> @location(0) vec4<f32> {
    let o = u.texel * 0.25;
    let c = (read(in.tex_coord + vec2<f32>(-o.x, -o.y)) + read(in.tex_coord + vec2<f32>(o.x, -o.y))
           + read(in.tex_coord + vec2<f32>(-o.x, o.y)) + read(in.tex_coord + vec2<f32>(o.x, o.y))) * 0.25;
    let b = min(max(c - vec3<f32>(u.threshold), vec3<f32>(0.0)), vec3<f32>(64.0));
    return vec4<f32>(b, 1.0);
}

// A nine-tap Gaussian two texels a step: a disc some thirty frame pixels
// across, which is what makes a ghost of a small light a soft blob.
fn blur(uv: vec2<f32>, step: vec2<f32>) -> vec4<f32> {
    var w = array<f32, 5>(0.2270, 0.1945, 0.1216, 0.0541, 0.0162);
    var c = read(uv) * w[0];
    for (var i = 1; i < 5; i = i + 1) {
        let o = step * f32(i) * 2.0;
        c = c + (read(uv + o) + read(uv - o)) * w[i];
    }
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_blur_h(in: VsOut) -> @location(0) vec4<f32> {
    return blur(in.tex_coord, vec2<f32>(u.texel.x, 0.0));
}

@fragment
fn fs_blur_v(in: VsOut) -> @location(0) vec4<f32> {
    return blur(in.tex_coord, vec2<f32>(0.0, u.texel.y));
}

// What is bright at `uv` - nothing off the screen.
fn bright(uv: vec2<f32>) -> vec3<f32> {
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec3<f32>(0.0);
    }
    return read(uv);
}

// A lens passes less from near its rim: ghosts of a light near the frame's
// edge are fainter, but not gone.
fn edge_fade(at: vec2<f32>) -> f32 {
    let d = length((vec2<f32>(0.5) - at) * vec2<f32>(u.aspect, 1.0));
    return pow(clamp(1.0 - d / 1.2, 0.0, 1.0), 1.5);
}

// Red, green and blue read a little apart along `dir`: a lens's fringes.
fn dispersed(uv: vec2<f32>, dir: vec2<f32>) -> vec3<f32> {
    let o = dir * u.dispersion;
    return vec3<f32>(bright(uv + o).r, bright(uv).g, bright(uv - o).b);
}

// Each ghost its own colour, as each element of a lens is coated differently.
fn ghost_tint(i: i32) -> vec3<f32> {
    switch (i % 4) {
        case 0: { return vec3<f32>(1.0, 0.75, 0.5); }
        case 1: { return vec3<f32>(0.5, 0.8, 1.0); }
        case 2: { return vec3<f32>(0.75, 1.0, 0.6); }
        default: { return vec3<f32>(1.0, 0.55, 0.85); }
    }
}

@fragment
fn fs_features(in: VsOut) -> @location(0) vec4<f32> {
    // Mirrored through the centre: a light at the top left flares to the
    // bottom right.
    let flipped = vec2<f32>(1.0) - in.tex_coord;
    let to_centre = (vec2<f32>(0.5) - flipped) * u.spacing;
    let dir = normalize(to_centre + vec2<f32>(1e-5));
    var flare = vec3<f32>(0.0);
    for (var i = 0; i < 8; i = i + 1) {
        if (f32(i) >= u.ghosts) {
            break;
        }
        let at = flipped + to_centre * f32(i);
        flare = flare + dispersed(at, dir) * edge_fade(at) * ghost_tint(i);
    }
    if (u.halo > 0.0) {
        // Read a halo's distance further towards the centre, and a little
        // either side of it.
        let h = vec2<f32>(dir.x / u.aspect, dir.y) * u.halo;
        var ring = vec3<f32>(0.0);
        for (var k = -2; k <= 2; k = k + 1) {
            let r = 1.0 + f32(k) * 0.5 * u.halo_width / max(u.halo, 1e-3);
            let at = flipped + h * r;
            ring = ring + dispersed(at, dir) * edge_fade(at) * (1.0 - abs(f32(k)) * 0.3);
        }
        flare = flare + ring * 0.25;
    }
    if (u.streak > 0.0) {
        // A long horizontal smear of whatever is bright on this row, taps
        // closer than the blur is wide, so it is a line and not a row of dots.
        var s = vec3<f32>(0.0);
        for (var k = 1; k <= 24; k = k + 1) {
            let off = f32(k) / 24.0 * u.streak_len;
            let w = 1.0 - f32(k) / 25.0;
            s = s + (bright(in.tex_coord + vec2<f32>(off, 0.0)) + bright(in.tex_coord - vec2<f32>(off, 0.0))) * w;
        }
        flare = flare + s * u.streak * vec3<f32>(0.6, 0.75, 1.0) / 12.0;
    }
    return vec4<f32>(flare, 1.0);
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let base = textureSampleLevel(src_texture, src_sampler, in.tex_coord, 0.0);
    let flare = textureSampleLevel(flare_texture, src_sampler, in.tex_coord, 0.0).rgb;
    return vec4<f32>(base.rgb + flare * u.intensity * u.tint.rgb, base.a);
}
