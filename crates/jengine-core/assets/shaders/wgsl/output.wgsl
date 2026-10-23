// Output - the last pass of every frame's post chain: scene -> display.
//
// The world is drawn into a float target (`WgpuRenderer::scene_format`),
// where light and additive glows may go past 1.0. The display cannot, so
// this pass maps the scene's range onto it:
//
//   exposure  multiplies the scene first (a photographer's stop);
//   tonemap   0 none     - clamp, which is what an 8-bit target always did;
//                          every scene that does not opt into HDR uses it;
//             1 reinhard - c / (1 + c); gentle, and desaturates highlights;
//             2 aces     - Stephen Hill's fit of the ACES RRT + ODT, the
//                          film look Unity and Godot ship; bright colours
//                          roll off toward white (fire goes yellow-white);
//             3 agx      - the minimal AgX fit (Benjamin Wrensch, after
//                          Troy Sobotka); keeps hue in bright colours better
//                          than ACES, Blender's and Godot 4.4's default.
//   grade     a colour look-up table (`renderer::lut`, `color_lut`), on the
//             tonemapped colour - what it was painted against - mixed in
//             by its strength;
//   fxaa      anti-aliasing (`fxaa`, Oct 2026): a stair-stepped edge is
//             found from the tonemapped brightness of a pixel's four
//             diagonal neighbours and blurred along itself (FXAA, Timothy
//             Lottes' console variant) - the neighbours are shaded exactly
//             as the pixel is, so it sees the edges the display shows. Not
//             for a pixel-perfect camera, whose stairs are the art;
//   rain      rain on the lens (`lens_rain`, Oct 2026): beads that form and
//             dry on the glass, and drops that run down in wiggling columns
//             leaving a trail of beads - all a height field worked out from
//             the clock, no state - the frame read through its slope, so
//             each drop bends what is behind it. `mist` steams the glass
//             (a soft blur, lifted) except where drops sit or have just
//             wiped it;
//   boil      line boil (`boil`, Oct 2026): the whole frame read through a
//             smooth noise offset of up to `amount` pixels, its pattern
//             `scale` pixels across, drawn afresh `rate` times a second - so
//             every line wobbles as if redrawn, as a hand-drawn cartoon's
//             do. First, so everything after reads the boiled frame;
//   paint     a painting (`paint`, Oct 2026): a Kuwahara filter - the four
//             squares of `radius` texels meeting at the pixel, and the mean
//             of the one whose colours vary least - so flat areas become
//             strokes and edges stay sharp; taps strided to at most 6 x 6 a
//             square;
//   halftone  a print screen (`halftone`, Oct 2026): dots on a turned grid
//             of `cell` pixels, each sized by the colour at its cell's
//             centre - one screen of `ink` on `paper` (mono), or cyan,
//             magenta, yellow and black at 15, 75, 0 and 45 degrees (cmyk).
//             The four nearest cells are tested, so dark dots merge. It
//             reads the frame before `paint`, not after;
//   grain     film grain (`film_grain`), in display space, a new pattern
//             every frame;
//   palette   the frame quantised to a palette (`palette`, Oct 2026;
//             `renderer::palette`, whose `quantize` is this on the CPU):
//             sRGB, nudged by a 4x4 ordered threshold, the nearest colour,
//             a cycle rotating some indices - exact colours, so the 8-bit
//             dither is off under it;
//   dither    +/- half an 8-bit step, from a per-pixel hash, so a dark
//             gradient does not band. Deterministic (no time term), so an
//             image does not shimmer and a golden stays stable.
//
// The destination is an sRGB format (or an sRGB view), so the hardware
// encodes on store: this returns linear colour.

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

struct OutputUniforms {
    exposure: f32,
    tonemap:  f32,
    dither:   f32,
    // A pixel-perfect camera (`renderer::pixel_perfect`): the scene is its
    // low-resolution target, read nearest from `source` (u0, v0, du, dv).
    nearest:  f32,
    source:   vec4<f32>,
    // x strength (0 none), y slices of the strip.
    lut:      vec4<f32>,
    // x intensity, y size in pixels, z this frame's seed.
    grain:    vec4<f32>,
    // FXAA: x on (1) or off, y the farthest it blurs along an edge, in
    // texels.
    aa:       vec4<f32>,
    // Palette: x count (0 none), y dither, z cell (px), w cycle shift.
    pal:      vec4<f32>,
    // The cycling indices: x first, y one past the last.
    cyc:      vec4<f32>,
    palette:  array<vec4<f32>, 16>,
    // Paint: x on, y radius in texels, z strength.
    paint:    vec4<f32>,
    // Halftone: x 0 off, 1 mono, 2 cmyk; y cell in pixels; z angle
    // (radians, mono); w strength.
    ht:       vec4<f32>,
    // Its ink and paper, linear.
    ink:      vec4<f32>,
    paper:    vec4<f32>,
    // Line boil: x amount (pixels, 0 off), y scale (pixels), z this
    // drawing's seed.
    boil:     vec4<f32>,
    // Lens rain: x how many drops (0 off), y drop size (pixels), z the
    // time (seconds), w how fast they run.
    rain:     vec4<f32>,
    // x mist, y refraction.
    rain2:    vec4<f32>,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: OutputUniforms;
// The colour grade's strip (a 1x1 stand-in when there is none).
@group(0) @binding(3) var lut_texture: texture_2d<f32>;

// ACES fitted (Stephen Hill, MJP's BakingLab). Input and output linear sRGB.
fn aces_fitted(c: vec3<f32>) -> vec3<f32> {
    // sRGB -> ACES AP1 (RRT_SAT), then back. The HLSL original's rows are
    // this constructor's columns (WGSL builds matrices from columns), so
    // `m * v` here is its `mul(M, v)`.
    let m_in = mat3x3<f32>(
        vec3<f32>(0.59719, 0.07600, 0.02840),
        vec3<f32>(0.35458, 0.90834, 0.13383),
        vec3<f32>(0.04823, 0.01566, 0.83777),
    );
    let m_out = mat3x3<f32>(
        vec3<f32>( 1.60475, -0.10208, -0.00327),
        vec3<f32>(-0.53108,  1.10813, -0.07276),
        vec3<f32>(-0.07367, -0.00605,  1.07602),
    );
    let v = m_in * c;
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return clamp(m_out * (a / b), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Minimal AgX (Benjamin Wrensch, 2023). Input linear sRGB, output linear.
fn agx_contrast(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4
        - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x - 0.00232;
}

fn agx(c: vec3<f32>) -> vec3<f32> {
    let inset = mat3x3<f32>(
        vec3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051),
        vec3<f32>(0.0784335999999992, 0.878468636469772, 0.0784336),
        vec3<f32>(0.0792237451477643, 0.0791661274605434, 0.879142973793104),
    );
    let outset = mat3x3<f32>(
        vec3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438),
        vec3<f32>(-0.0980208811401368, 1.15190312990417, -0.0980434501171241),
        vec3<f32>(-0.0990297440797205, -0.0989611768448433, 1.15107367264116),
    );
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var v = inset * max(c, vec3<f32>(1e-10));
    v = clamp(log2(v), vec3<f32>(min_ev), vec3<f32>(max_ev));
    v = (v - min_ev) / (max_ev - min_ev);
    v = agx_contrast(v);
    v = outset * v;
    // The fit produces display-encoded values; decode to linear for the
    // sRGB target to re-encode.
    return pow(max(v, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn tonemap(c: vec3<f32>, mode: i32) -> vec3<f32> {
    switch mode {
        case 1: { return c / (vec3<f32>(1.0) + c); }
        case 2: { return aces_fitted(c); }
        case 3: { return agx(c); }
        default: { return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)); }
    }
}

// Linear to display (sRGB) encoding, exactly - the strip was painted there.
fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// A colour through the strip of `n` slices (red right, green down, blue
// slice by slice), blending the two slices either side of blue. Returns
// linear colour: the strip is an sRGB texture. `renderer::lut::strip_uv` is
// this on the CPU.
fn lut_lookup(c: vec3<f32>, n: f32) -> vec3<f32> {
    let s = clamp(srgb_encode(c), vec3<f32>(0.0), vec3<f32>(1.0));
    let blue = s.b * (n - 1.0);
    let b0 = floor(blue);
    let b1 = min(b0 + 1.0, n - 1.0);
    let x = s.r * (n - 1.0) + 0.5;
    let y = (s.g * (n - 1.0) + 0.5) / n;
    let a = textureSampleLevel(lut_texture, src_sampler, vec2<f32>((b0 * n + x) / (n * n), y), 0.0).rgb;
    let b = textureSampleLevel(lut_texture, src_sampler, vec2<f32>((b1 * n + x) / (n * n), y), 0.0).rgb;
    return mix(a, b, blue - b0);
}

// A cheap hash of the pixel position, 0..1.
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// The scene at `t`, as the display shows it: exposed and tonemapped.
fn shade(t: vec2<f32>) -> vec3<f32> {
    let s = textureSampleLevel(src_texture, src_sampler, t, 0.0).rgb;
    let raw = select(vec3<f32>(0.0), s, s == s);
    return tonemap(max(raw, vec3<f32>(0.0)) * u.exposure, i32(u.tonemap + 0.5));
}

// Brightness as the eye reads the display (roughly sRGB-encoded luma).
fn luma(c: vec3<f32>) -> f32 {
    return dot(sqrt(max(c, vec3<f32>(0.0))), vec3<f32>(0.299, 0.587, 0.114));
}

// FXAA (Timothy Lottes, the console variant): `m` is the pixel's own shade
// at `t`. An edge is where the four diagonal neighbours disagree; the blur
// runs along it, at most `u.aa.y` texels, and falls back to a shorter one
// if the long sample crosses into something else.
fn fxaa(t: vec2<f32>, m: vec3<f32>) -> vec3<f32> {
    let px = 1.0 / vec2<f32>(textureDimensions(src_texture));
    let nw = luma(shade(t + vec2<f32>(-1.0, -1.0) * px));
    let ne = luma(shade(t + vec2<f32>(1.0, -1.0) * px));
    let sw = luma(shade(t + vec2<f32>(-1.0, 1.0) * px));
    let se = luma(shade(t + vec2<f32>(1.0, 1.0) * px));
    let lm = luma(m);
    let lo = min(lm, min(min(nw, ne), min(sw, se)));
    let hi = max(lm, max(max(nw, ne), max(sw, se)));
    // Flat, or too faint a step to be a visible stair.
    if (hi - lo < max(0.0312, hi * 0.125)) {
        return m;
    }
    var dir = vec2<f32>(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
    let reduce = max((nw + ne + sw + se) * 0.25 * (1.0 / 8.0), 1.0 / 128.0);
    let rcp = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * rcp, vec2<f32>(-u.aa.y), vec2<f32>(u.aa.y)) * px;
    let a = 0.5 * (shade(t + dir * (1.0 / 3.0 - 0.5)) + shade(t + dir * (2.0 / 3.0 - 0.5)));
    let b = a * 0.5 + 0.25 * (shade(t + dir * -0.5) + shade(t + dir * 0.5));
    let lb = luma(b);
    return select(b, a, lb < lo || lb > hi);
}

// Smooth value noise, 0..1.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash12(i), hash12(i + vec2<f32>(1.0, 0.0)), s.x),
               mix(hash12(i + vec2<f32>(0.0, 1.0)), hash12(i + vec2<f32>(1.0, 1.0)), s.x), s.y);
}

// Line boil's offset at pixel `p`, in pixels: smooth over `u.boil.y`, a new
// pattern for each seed.
fn boil_offset(p: vec2<f32>) -> vec2<f32> {
    let q = p / max(u.boil.y, 1.0) + vec2<f32>(u.boil.z * 13.17, u.boil.z * 7.31);
    return (vec2<f32>(vnoise(q), vnoise(q + vec2<f32>(31.7, 11.3))) - vec2<f32>(0.5)) * 2.0 * u.boil.x;
}

// A drop's height at distance `d` from its middle: a cap of radius `r`.
fn drop_cap(d: f32, r: f32) -> f32 {
    if (r <= 0.0) {
        return 0.0;
    }
    let x = clamp(d / r, 0.0, 1.0);
    return sqrt(1.0 - x * x);
}

// Lens rain at pixel `p` (y down): x how wet the glass is (the tallest
// drop there, 0 to 1), y how freshly a running drop wiped it.
fn rain_at(p: vec2<f32>) -> vec2<f32> {
    let size = max(u.rain.y, 2.0);
    let time = u.rain.z;
    var h = 0.0;
    var wipe = 0.0;
    // Beads: one a cell, each forming and drying on its own clock.
    let cell = size * 2.6;
    let id = floor(p / cell);
    let r1 = hash12(id + vec2<f32>(3.1, 7.7));
    let r2 = hash12(id + vec2<f32>(11.3, 1.9));
    let r3 = hash12(id + vec2<f32>(5.7, 13.1));
    if (r3 < u.rain.x) {
        let c = (id + vec2<f32>(0.25) + vec2<f32>(r1, r2) * 0.5) * cell;
        let life = fract(time * (0.04 + 0.08 * r2) + r1);
        let grow = smoothstep(0.0, 0.2, life) * (1.0 - smoothstep(0.75, 1.0, life));
        h = max(h, drop_cap(length(p - c), size * (0.2 + 0.3 * r2) * grow));
    }
    // Runners: a column each, its drop sliding down, a trail behind it.
    let colw = size * 4.0;
    let col = floor(p.x / colw);
    let k1 = hash12(vec2<f32>(col, 1.7));
    let k2 = hash12(vec2<f32>(col, 9.3));
    let k3 = hash12(vec2<f32>(col, 4.1));
    if (k1 < u.rain.x * 0.8) {
        let period = size * 80.0;
        let speed = size * (5.0 + 12.0 * k2) * max(u.rain.w, 0.0);
        let head0 = fract(time * speed / period + k3) * period;
        let hy = head0 + period * round((p.y - head0) / period);
        let x = (col + 0.5) * colw + sin(p.y / (size * 3.0) + k3 * 6.283) * colw * 0.14;
        let dx = p.x - x;
        let dy = p.y - hy;
        h = max(h, drop_cap(length(vec2<f32>(dx, dy * 0.8)), size));
        let trail = size * 16.0;
        if (dy < 0.0 && dy > -trail) {
            let along = -dy / trail;
            wipe = max(wipe, (1.0 - along) * smoothstep(size * 0.9, size * 0.35, abs(dx)));
            let bead = size * 1.6;
            let bi = floor(dy / bead);
            let by = (bi + 0.5) * bead;
            let rb = size * 0.32 * (1.0 - along) * step(0.4, hash12(vec2<f32>(col, bi)));
            h = max(h, drop_cap(length(vec2<f32>(dx, dy - by)), rb));
        }
    }
    return vec2<f32>(h, wipe);
}

// How far lens rain bends the frame at pixel `p` (xy, pixels), how clear
// the glass is there (z: a drop, or a fresh wipe) and how a drop's surface
// catches the light there (w: + a glint on the side facing up and left,
// - a darker rim).
fn rain_bend(p: vec2<f32>) -> vec4<f32> {
    let here = rain_at(p);
    let gx = rain_at(p + vec2<f32>(1.0, 0.0)).x - rain_at(p - vec2<f32>(1.0, 0.0)).x;
    let gy = rain_at(p + vec2<f32>(0.0, 1.0)).x - rain_at(p - vec2<f32>(0.0, 1.0)).x;
    let size = max(u.rain.y, 2.0);
    let g = vec2<f32>(gx, gy) * 0.5 * size;
    let bend = -g * u.rain2.y;
    let clear = max(smoothstep(0.02, 0.25, here.x), here.y);
    let slope = length(g);
    let glint = smoothstep(0.55, 0.95, dot(-g / max(slope, 1e-4), vec2<f32>(-0.6, -0.8))) * smoothstep(0.2, 0.8, slope);
    let rim = smoothstep(0.9, 2.5, slope);
    let light = (glint * 0.35 - rim * 0.3) * min(u.rain2.y, 1.0) * step(0.001, here.x);
    return vec4<f32>(bend, clear, light);
}

// The steamed glass at `t`: the frame softened over `r` pixels and lifted.
fn mist_at(t: vec2<f32>, step: vec2<f32>, r: f32) -> vec3<f32> {
    var acc = shade(t);
    for (var k = 0; k < 8; k = k + 1) {
        let a = f32(k) * 0.785398;
        acc = acc + shade(t + vec2<f32>(cos(a), sin(a)) * r * step);
    }
    return acc / 9.0 * 0.88 + vec3<f32>(0.03);
}

// Kuwahara: of the four squares of `u.paint.y` texels meeting at `t`, the
// mean colour of the one that varies least.
fn paint(t: vec2<f32>) -> vec3<f32> {
    let px = 1.0 / vec2<f32>(textureDimensions(src_texture));
    let r = max(i32(u.paint.y + 0.5), 1);
    let stride = max(1, (r + 6) / 6);
    var best = 1e9;
    var out = vec3<f32>(0.0);
    for (var q = 0; q < 4; q = q + 1) {
        let s = vec2<f32>(select(-1.0, 1.0, (q & 1) == 1), select(-1.0, 1.0, (q & 2) == 2));
        var m = vec3<f32>(0.0);
        var m2 = vec3<f32>(0.0);
        var n = 0.0;
        for (var j = 0; j <= r; j = j + stride) {
            for (var i = 0; i <= r; i = i + stride) {
                let c = shade(t + vec2<f32>(f32(i), f32(j)) * s * px);
                m = m + c;
                m2 = m2 + c * c;
                n = n + 1.0;
            }
        }
        m = m / n;
        let v = m2 / n - m * m;
        let spread = v.r + v.g + v.b;
        if (spread < best) {
            best = spread;
            out = m;
        }
    }
    return out;
}

// The share of ink channel `ch` (0..3 cyan, magenta, yellow, black; 4 the
// mono screen's) a colour `s` (display space) wants.
fn ink_share(s: vec3<f32>, ch: i32) -> f32 {
    if (ch == 4) {
        return 1.0 - dot(s, vec3<f32>(0.299, 0.587, 0.114));
    }
    let k = 1.0 - max(s.r, max(s.g, s.b));
    if (ch == 3) {
        return k;
    }
    let d = max(1.0 - k, 1e-4);
    if (ch == 0) {
        return (1.0 - s.r - k) / d;
    }
    if (ch == 1) {
        return (1.0 - s.g - k) / d;
    }
    return (1.0 - s.b - k) / d;
}

// How much of the pixel at `pp` one screen covers: a grid of `u.ht.y`
// pixels turned by `a`, each dot sized by the share of channel `ch` at its
// cell's centre (`step` takes a pixel to the source's texture coordinates).
fn screen_cover(pp: vec2<f32>, step: vec2<f32>, a: f32, ch: i32) -> f32 {
    let cell = max(u.ht.y, 2.0);
    let cs = cos(a);
    let sn = sin(a);
    let g = vec2<f32>(pp.x * cs + pp.y * sn, -pp.x * sn + pp.y * cs) / cell;
    let b = floor(g - vec2<f32>(0.5));
    var cover = 0.0;
    for (var k = 0; k < 4; k = k + 1) {
        let cc = (b + vec2<f32>(f32(k & 1), f32(k >> 1u)) + vec2<f32>(0.5)) * cell;
        let cp = vec2<f32>(cc.x * cs - cc.y * sn, cc.x * sn + cc.y * cs);
        let s = pow(max(shade(u.source.xy + cp * step), vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
        let amt = clamp(ink_share(s, ch), 0.0, 1.0);
        // Area for area up to touching dots, then grown to fill the corners.
        let r = cell * mix(sqrt(amt / 3.14159265), 0.72, smoothstep(0.7, 1.0, amt));
        let d = length(g * cell - cc);
        cover = max(cover, clamp(r - d + 0.5, 0.0, 1.0));
    }
    return cover;
}

// The halftone print of the pixel at `pp`, linear.
fn halftone(pp: vec2<f32>, step: vec2<f32>) -> vec3<f32> {
    if (u.ht.x < 1.5) {
        return mix(u.paper.rgb, u.ink.rgb, screen_cover(pp, step, u.ht.z, 4));
    }
    let deg = 0.017453293;
    var s = pow(max(u.paper.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    s = s * (vec3<f32>(1.0) - vec3<f32>(screen_cover(pp, step, 15.0 * deg, 0), 0.0, 0.0));
    s = s * (vec3<f32>(1.0) - vec3<f32>(0.0, screen_cover(pp, step, 75.0 * deg, 1), 0.0));
    s = s * (vec3<f32>(1.0) - vec3<f32>(0.0, 0.0, screen_cover(pp, step, 0.0, 2)));
    let lin = pow(s, vec3<f32>(2.2));
    return mix(lin, u.ink.rgb, screen_cover(pp, step, 45.0 * deg, 3));
}

// The palette's nearest colour to `c` (linear) at pixel `p` - see
// `renderer::palette::quantize`, which is this line for line.
fn palettize(c: vec3<f32>, p: vec2<f32>) -> vec3<f32> {
    var bayer = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let cell = vec2<i32>(floor(p / u.pal.z));
    let bx = ((cell.x % 4) + 4) % 4;
    let by = ((cell.y % 4) + 4) % 4;
    let t = bayer[by * 4 + bx] / 16.0 + 1.0 / 32.0 - 0.5;
    let s = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)) + vec3<f32>(t * u.pal.y * 0.25);
    let n = i32(u.pal.x + 0.5);
    var best = 1e9;
    var bi = 0;
    for (var i = 0; i < 16; i = i + 1) {
        if (i >= n) {
            break;
        }
        let d = s - u.palette[i].rgb;
        let e = d.r * d.r * 0.30 + d.g * d.g * 0.59 + d.b * d.b * 0.11;
        if (e < best) {
            best = e;
            bi = i;
        }
    }
    let a = i32(u.cyc.x + 0.5);
    let b = i32(u.cyc.y + 0.5);
    if (b > a + 1 && bi >= a && bi < b) {
        bi = a + (bi - a + i32(u.pal.w + 0.5)) % (b - a);
    }
    return pow(u.palette[bi].rgb, vec3<f32>(2.2));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var t = u.source.xy + in.tex_coord * u.source.zw;
    // One output pixel in the source's texture coordinates (a halftone
    // cell is measured on the display) - taken here, in uniform control.
    let step = vec2<f32>(abs(dpdx(in.tex_coord.x)), abs(dpdy(in.tex_coord.y))) * u.source.zw;
    if (u.boil.x > 0.0) {
        t = t + boil_offset(in.clip_position.xy) * step;
    }
    // Lens rain bends what is behind each drop.
    var wet = vec4<f32>(0.0, 0.0, 1.0, 0.0);
    if (u.rain.x > 0.0) {
        wet = rain_bend(in.clip_position.xy);
        t = t + wet.xy * step;
    }
    var src = textureSample(src_texture, src_sampler, t);
    if (u.nearest > 0.5) {
        let dims = vec2<f32>(textureDimensions(src_texture));
        let texel = clamp(floor(t * dims), vec2<f32>(0.0), dims - vec2<f32>(1.0));
        src = textureLoad(src_texture, vec2<i32>(texel), 0);
    }
    // `c == c` is false for NaN, which a float target can hold and a
    // display cannot show.
    let raw = select(vec3<f32>(0.0), src.rgb, src.rgb == src.rgb);
    var c = tonemap(max(raw, vec3<f32>(0.0)) * u.exposure, i32(u.tonemap + 0.5));
    if (u.aa.x > 0.5 && u.nearest < 0.5) {
        c = fxaa(t, c);
    }
    if (u.rain.x > 0.0 && u.rain2.x > 0.0) {
        c = mix(c, mist_at(t, step, max(u.rain.y, 2.0) * 1.2), u.rain2.x * (1.0 - wet.z));
    }
    // A drop's glint and rim.
    if (u.rain.x > 0.0) {
        c = max(c * (1.0 + min(wet.w, 0.0)) + vec3<f32>(max(wet.w, 0.0)), vec3<f32>(0.0));
    }
    if (u.paint.x > 0.5) {
        c = mix(c, paint(t), u.paint.z);
    }
    if (u.ht.x > 0.5) {
        c = mix(c, halftone(in.clip_position.xy, step), u.ht.w);
    }

    if (u.lut.x > 0.0) {
        c = mix(c, lut_lookup(c, u.lut.y), u.lut.x);
    }
    if (u.grain.x > 0.0) {
        // In display space, a cell of `size` pixels, a new pattern a frame;
        // gentler in the highlights, as film is.
        let cell = floor(in.clip_position.xy / u.grain.y);
        let n = hash12(cell + vec2<f32>(u.grain.z * 7.31, u.grain.z * 3.17)) - 0.5;
        var s = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
        let lum = dot(s, vec3<f32>(0.299, 0.587, 0.114));
        s = s + vec3<f32>(n * u.grain.x * (1.0 - 0.5 * lum));
        c = pow(max(s, vec3<f32>(0.0)), vec3<f32>(2.2));
    }
    if (u.pal.x > 0.5) {
        c = palettize(c, in.clip_position.xy);
    }

    if (u.dither > 0.0) {
        // Dither in display space, where 8-bit steps are even: encode
        // approximately, nudge by up to half a step either way, decode.
        let n = hash12(in.clip_position.xy) + hash12(in.clip_position.xy + vec2<f32>(17.0, 59.0)) - 1.0;
        var s = pow(c, vec3<f32>(1.0 / 2.2));
        s = s + vec3<f32>(n * u.dither / 255.0);
        // Pure black stays black: a dithered void reads as noise.
        let keep = select(1.0, 0.0, max(c.r, max(c.g, c.b)) <= 0.0);
        c = mix(c, pow(max(s, vec3<f32>(0.0)), vec3<f32>(2.2)), keep);
    }
    return vec4<f32>(c, src.a);
}
