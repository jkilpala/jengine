// CRT - an old television's picture: a screen curved like glass, dark gaps
// between scanlines, and an aperture-grille mask of red, green and blue
// stripes. Best after `pixelate`, with `lines` equal to its, so each art row
// is one scanline.
//
// Uniforms (16 bytes):
//   lines     - scanlines down the screen.
//   scanline  - how dark the gap between two lines is (0 none, 1 black).
//   mask      - strength of the RGB stripes (0 none).
//   curvature - how far the corners bend away (0 flat; 0.05..0.1 a tube).

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

struct CrtUniforms {
    lines:     f32,
    scanline:  f32,
    mask:      f32,
    curvature: f32,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: CrtUniforms;

const PI: f32 = 3.14159265;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // The glass: a point `r` from the middle shows what is a little further
    // out, so the picture bulges and its corners fall off the tube.
    let c = in.tex_coord * 2.0 - 1.0;
    let bent = c * (1.0 + u.curvature * dot(c, c));
    let uv = bent * 0.5 + 0.5;
    // A soft edge where the picture ends, a pixel or two wide.
    let px = 2.0 / vec2<f32>(textureDimensions(src_texture));
    let inside = smoothstep(vec2<f32>(0.0), px, uv) * smoothstep(vec2<f32>(0.0), px, vec2<f32>(1.0) - uv);
    let edge = inside.x * inside.y;

    var col = textureSample(src_texture, src_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0))).rgb;

    // Scanlines: bright along each line's middle, dark in the gap.
    let f = fract(uv.y * max(u.lines, 1.0));
    let s = sin(PI * f);
    let scan = 1.0 - u.scanline * (1.0 - s * s);

    // The aperture grille, by screen column: each column favours one of
    // red, green and blue.
    let column = i32(in.clip_position.x) % 3;
    var stripe = vec3<f32>(1.0 - u.mask);
    if column == 0 { stripe.x = 1.0; } else if column == 1 { stripe.y = 1.0; } else { stripe.z = 1.0; }

    // Win back what the gaps and stripes take, so the picture is as bright
    // on the whole as it was.
    let gain = 1.0 / max((1.0 - u.scanline * 0.5) * (1.0 - u.mask * 2.0 / 3.0), 0.2);
    col = col * scan * stripe * gain;
    return vec4<f32>(col * edge, 1.0);
}
