
// ── Specular highlights (Oct 2026) ───────────────────────────────────────
// Appended to both light shaders. The normal pass writes each surface's
// specular strength and shininess beside its normal (`fs_normal`'s second
// output: r strength, g shininess as 0..1 of 1..128); a light adds a
// Blinn-Phong highlight on top of its diffuse response, the viewer straight
// above the screen and the light `height * radius` above the plane. A surface
// with no specular has strength 0 and is lit exactly as before.
@group(3) @binding(3) var u_specular: texture_2d<f32>;

// Each pixel's light layers (`LightLayers`, Oct 2026), written whole by the
// normal pass and cleared to every layer: 1 when this light (`light_mask`)
// lights the pixel's layers, 0 when they share no bit.
@group(3) @binding(4) var u_layers: texture_2d<u32>;

fn on_light_layers(frag: vec4<f32>) -> f32 {
    let here = textureLoad(u_layers, vec2<i32>(frag.xy), 0).x;
    return select(0.0, 1.0, (here & u_light.layers.x) != 0u);
}

// A light's two outputs: its diffuse into the light buffer, which the
// composite multiplies the scene by, and its highlight into the specular
// light buffer, which the composite adds on top.
struct LightOut {
    @location(0) light:    vec4<f32>,
    @location(1) specular: vec4<f32>,
};

// The highlight this light puts on the surface under `frag`, as a multiple
// of the light's own colour - added to `normal_response`'s diffuse.
fn specular_response(frag: vec4<f32>, world_pos: vec2<f32>, center: vec2<f32>) -> f32 {
    let s = textureLoad(u_specular, vec2<i32>(frag.xy), 0);
    if (s.r <= 0.0) {
        return 0.0;
    }
    var enc = textureLoad(u_normals, vec2<i32>(frag.xy), 0).xyz;
    // A caster's z is written as `(z - 0.5) * 0.9` (`fs_normal`).
    if (enc.z < 0.475) {
        enc.z = enc.z / 0.9 + 0.5;
    }
    let n = normalize(enc * 2.0 - vec3<f32>(1.0));
    // The diffuse reads `height` as the tangent of the light's elevation,
    // the same at every distance - fine for shading a bump, but a mirror
    // reflection at one angle everywhere is a glint nowhere. Here the light
    // hangs `height * radius` above the plane, so the highlight peaks under
    // it and spreads with its reach.
    let d = center - world_pos;
    let lift = max(u_light.extra.x * u_light.pos_radius_falloff.z, 1e-3);
    let l = normalize(vec3<f32>(d, lift));
    // Only a face the light reaches can shine.
    if (dot(n, l) <= 0.0) {
        return 0.0;
    }
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let shininess = mix(1.0, 128.0, s.g);
    return s.r * pow(max(dot(n, h), 0.0), shininess);
}
