
// ---- The light's cookie (light_cookie.wgsl, appended to both lights) ----
//
// A texture the light shines through (`PointLight2D::cookie`): its picture
// spans `reach` each way from the light (`u_light.extra.w`, 0 for no
// cookie), turned by `turn`, and multiplies the light - white passes, black
// stops, alpha fades. Outside the picture the light gives nothing.
// `renderer::lighting::cookie_factor` is this on the CPU, line for line.
// Bound beside the normals in group 3: WebGPU guarantees four bind groups
// and the lights use all four.
@group(3) @binding(1) var u_cookie: texture_2d<f32>;
@group(3) @binding(2) var u_cookie_sampler: sampler;

fn cookie_factor(p: vec2<f32>, center: vec2<f32>, turn: f32, reach: f32) -> vec3<f32> {
    if (reach <= 0.0) {
        return vec3<f32>(1.0);
    }
    let d = p - center;
    let s = sin(turn);
    let c = cos(turn);
    let local = vec2<f32>(d.x * c + d.y * s, -d.x * s + d.y * c) / reach;
    if (abs(local.x) > 1.0 || abs(local.y) > 1.0) {
        return vec3<f32>(0.0);
    }
    let t = textureSampleLevel(u_cookie, u_cookie_sampler, vec2<f32>(local.x * 0.5 + 0.5, 0.5 - local.y * 0.5), 0.0);
    return t.rgb * t.a;
}
