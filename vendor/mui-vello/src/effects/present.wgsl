// The two fragment passes around Vello's compute output: a separable Gaussian
// for backdrops, and the present that puts the frame on the surface. Vello
// stores straight alpha; what leaves here is premultiplied unless the surface
// composites straight alpha.

struct Blur {
    dir: vec2<i32>,
    radius: i32,
    sigma: f32,
    // Nonzero: the source is Vello's straight alpha.
    premultiply: u32,
    // The present's: the frame's size in the source, which only grows.
    frame: vec2<u32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> blur: Blur;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole target.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn premul(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

@fragment
fn fs_blur(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(p.xy);
    let last = vec2<i32>(textureDimensions(src)) - 1;
    let k = -0.5 / (blur.sigma * blur.sigma);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var i = -blur.radius; i <= blur.radius; i++) {
        // Clamped: the edge pixel repeats, as `EdgeMode::Duplicate` does.
        var c = textureLoad(src, clamp(at + blur.dir * i, vec2<i32>(0), last), 0);
        if blur.premultiply != 0u {
            c = premul(c);
        }
        let w = exp(k * f32(i * i));
        sum += c * w;
        weight += w;
    }
    return sum / weight;
}

// The frame's pixel under `p`, straight alpha, or clear past its edge: a
// retained source may be larger than the frame, and past it the
// source holds an older, larger frame, or nothing, which a translucent
// window would show.
fn frame(p: vec4<f32>) -> vec4<f32> {
    let at = vec2<u32>(p.xy);
    if any(at >= min(blur.frame, textureDimensions(src))) {
        return vec4<f32>(0.0);
    }
    return textureLoad(src, at, 0);
}

@fragment
fn fs_present(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return premul(frame(p));
}

// For a surface that composites straight alpha (`PostMultiplied`).
@fragment
fn fs_present_straight(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return frame(p);
}
