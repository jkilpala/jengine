// Phase 3.c-4 Step 6c — spot light WGSL.
//
// WGSL twin of `assets/shaders/spot_light.frag` (vertex stage matches
// `point_light.wgsl::vs_main` since spot lights also live in a quad
// scaled by their radius). Emits an attenuation disc masked by a soft
// cone facing `spot.x` with half-angle `spot.y`, with cast shadows from
// the shared occluder set (hard or soft via `spot.z`).
//
// Shares the same `Light` uniform layout as the point light pipeline,
// so the caller writes one combined dynamic-offset buffer for both.
//
// Bind groups:
//   @group(0) @binding(0) — view_projection (uniform mat4)
//   @group(1) @binding(0) — per-light Light uniform (dynamic offset)
//   @group(2) @binding(0) — occluder segments (uniform, shared)
//   @group(3) @binding(0) — the normal buffer (see `normal_response`)
//   @group(3) @binding(1, 2) — the light's cookie (`light_cookie.wgsl`)

struct Light {
    pos_radius_falloff: vec4<f32>,
    color_intensity:    vec4<f32>,
    // x = spot_direction (rad), y = spot_angle (rad), z = shadow softness,
    // w = cast-shadows flag (>0.5 ⇒ run the occluder test; 0 ⇒ no occluder
    // in reach, skip the loop — set per-light on the CPU as a broad-phase
    // cull).
    spot:               vec4<f32>,
    // x = start, y = count: this light's own range of the occluder pool,
    // packed on the CPU by `renderer::lighting::pack_light_occluders` - only
    // the edges within its reach, nearest first.
    occ: vec4<u32>,
    // x = height: the tangent of the light's elevation, for normal maps
    // (`PointLight2D::height`, 0.5 by default); y = the most a face turned
    // to the light may take relative to flat (`MAX_NORMAL_RESPONSE`).
    extra: vec4<f32>,
    // x = the light layers it lights (`light_mask`): a pixel whose layers
    // (`on_light_layers`) share no bit takes nothing from it. y = the bits
    // of its squash (`SpotLight2D::squash`).
    layers: vec4<u32>,
};

// The occluder pool and the shadow functions are `light_shadow.wgsl`,
// appended to this file.

@group(0) @binding(0) var<uniform> u_view_projection: mat4x4<f32>;
@group(1) @binding(0) var<uniform> u_light: Light;
@group(2) @binding(0) var<uniform> u_occ: Occluders;

// The normal buffer: the world's surface normals, drawn by
// `render_normals_wgpu` before the lights, encoded 0..1 (+y up). Flat
// (0.5, 0.5, 1) wherever nothing has a normal map - which is every pixel
// of a scene with none, so the lights below light it exactly as they did
// before normal maps joined this pass (Sep 2026). They used to be a forward
// pass of their own, drawn over every other sprite whatever its z.
@group(3) @binding(0) var u_normals: texture_2d<f32>;

// How much of this light the surface under `frag` takes **relative to a flat
// surface**: 1.0 for flat, more for a bump facing the light (up to 1/l.z),
// 0 for one facing away. Relative, so a scene without normal maps is lit
// exactly as it was, and the CPU's `light_at` still answers for it.
fn normal_response(frag: vec4<f32>, world_pos: vec2<f32>, center: vec2<f32>) -> f32 {
    var enc = textureLoad(u_normals, vec2<i32>(frag.xy), 0).xyz;
    // A caster's z is written as `(z - 0.5) * 0.9` (`fs_normal`).
    if (enc.z < 0.475) {
        enc.z = enc.z / 0.9 + 0.5;
    }
    let n = normalize(enc * 2.0 - vec3<f32>(1.0));
    let d = center - world_pos;
    let len = length(d);
    let dir = select(d / max(len, 1e-4), vec2<f32>(0.0), len < 1e-3);
    // The light's own height (`u_light.extra.x`): lower is more grazing.
    let l = normalize(vec3<f32>(dir, u_light.extra.x));
    return min(max(dot(n, l), 0.0) / l.z, u_light.extra.y);
}

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       world_pos:     vec2<f32>,
};

@vertex
fn vs_main(@location(0) a_pos: vec2<f32>) -> VsOut {
    let radius = u_light.pos_radius_falloff.z;
    let center = u_light.pos_radius_falloff.xy;
    let world  = a_pos * radius + center;
    var out: VsOut;
    out.clip_position = u_view_projection * vec4<f32>(world, 0.0, 1.0);
    out.world_pos     = world;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> LightOut {
    let center    = u_light.pos_radius_falloff.xy;
    let radius    = u_light.pos_radius_falloff.z;
    let falloff   = u_light.pos_radius_falloff.w;
    let color     = u_light.color_intensity.rgb;
    let intensity = u_light.color_intensity.a;
    let spot_dir  = u_light.spot.x;
    let spot_ang  = u_light.spot.y;
    let softness  = u_light.spot.z;
    let cast_shadows = u_light.spot.w;

    let delta       = in.world_pos - center;
    let dist        = length(delta);
    // The falloff squashed top to bottom (`SpotLight2D::squash`); the cone
    // is still the light's own direction.
    let s           = bitcast<f32>(u_light.layers.y);
    let squash      = select(1.0, clamp(s, 0.05, 1.0), s > 0.0);
    var attenuation = 1.0 - clamp(length(vec2<f32>(delta.x, delta.y / squash)) / radius, 0.0, 1.0);
    attenuation     = pow(attenuation, falloff);

    let dir       = vec2<f32>(cos(spot_dir), sin(spot_dir));
    // Guard against normalize(0) at the light center.
    let safe_norm = select(normalize(delta), dir, dist < 0.001);
    let cos_frag  = dot(safe_norm, dir);
    let cos_cone  = cos(spot_ang);
    var cone      = smoothstep(cos_cone - 0.05, cos_cone + 0.05, cos_frag);
    if (dist < 0.001) {
        cone = 1.0;
    }

    // Per-light broad-phase cull: skip the occluder loop entirely when
    // the CPU found no occluder within this light's radius (uniform
    // branch → effectively free).
    var shadow = vec3<f32>(1.0);
    if (cast_shadows > 0.5) {
        shadow = shadow_factor(in.world_pos, center, softness, is_caster(in.clip_position));
    }

    let bump = normal_response(in.clip_position, in.world_pos, center);
    // Through the cookie, turned with the spot's aim.
    let through = cookie_factor(in.world_pos, center, spot_dir, u_light.extra.w);
    // The highlight on a shiny surface (`light_specular.wgsl`), over the diffuse.
    let shine = specular_response(in.clip_position, in.world_pos, center);
    let lit = color * intensity * attenuation * cone * shadow * through * on_light_layers(in.clip_position);
    var out: LightOut;
    out.light = vec4<f32>(lit * bump, 1.0);
    out.specular = vec4<f32>(lit * shine, 1.0);
    return out;
}
