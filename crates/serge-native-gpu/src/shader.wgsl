// serge-native-gpu: every primitive of a frame is one instance of one
// pipeline, drawn with one draw call. Colors are sRGB-encoded and blended
// premultiplied, like the CPU path.

struct Globals {
    size: vec2<f32>,
    pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
// Glyph masks (premultiplied RGBA: white coverage, or a color glyph).
@group(0) @binding(1) var glyph_atlas: texture_2d<f32>;
// This frame's path masks (coverage in red).
@group(0) @binding(2) var path_atlas: texture_2d<f32>;

const FILL: f32 = 0.0;
const BORDER: f32 = 1.0;
const GLYPH: f32 = 2.0;
const COLOR_GLYPH: f32 = 3.0;
const PATH: f32 = 4.0;

struct Instance {
    // x, y, width, height in device pixels.
    @location(0) rect: vec4<f32>,
    // Straight-alpha color, sRGB-encoded, 0..1.
    @location(1) color: vec4<f32>,
    // Clip rectangle: x0, y0, x1, y1.
    @location(2) clip: vec4<f32>,
    // Fill and border: radius, border width. Masks: atlas x, y. Then kind.
    @location(3) extra: vec4<f32>,
};

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) rect: vec4<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
    @location(3) @interpolate(flat) extra: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, inst: Instance) -> Varyings {
    let corner = vec2<f32>(f32(vertex & 1u), f32(vertex >> 1u));
    var lo = inst.rect.xy;
    var hi = inst.rect.xy + inst.rect.zw;
    if inst.extra.z < GLYPH - 0.5 {
        // Room for the anti-aliased edge.
        lo = lo - vec2<f32>(1.0);
        hi = hi + vec2<f32>(1.0);
    }
    lo = max(lo, floor(inst.clip.xy));
    hi = max(min(hi, ceil(inst.clip.zw)), lo);
    let p = mix(lo, hi, corner);
    var out: Varyings;
    out.position = vec4<f32>(
        p.x / globals.size.x * 2.0 - 1.0,
        1.0 - p.y / globals.size.y * 2.0,
        0.0,
        1.0,
    );
    out.rect = inst.rect;
    out.color = inst.color;
    out.clip = inst.clip;
    out.extra = inst.extra;
    return out;
}

// The share of the pixel centered at `p` inside [lo, hi].
fn box_coverage(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>) -> f32 {
    let a = clamp(min(p + 0.5, hi) - max(p - 0.5, lo), vec2<f32>(0.0), vec2<f32>(1.0));
    return a.x * a.y;
}

fn rounded_coverage(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, radius: f32) -> f32 {
    let half = (hi - lo) * 0.5;
    let r = min(radius, min(half.x, half.y));
    if r <= 0.0 {
        return box_coverage(p, lo, hi);
    }
    let q = abs(p - (lo + half)) - half + vec2<f32>(r);
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    return min(clamp(0.5 - d, 0.0, 1.0), box_coverage(p, lo, hi));
}

@fragment
fn fs_main(v: Varyings) -> @location(0) vec4<f32> {
    let p = v.position.xy;
    let kind = v.extra.z;
    let clip = box_coverage(p, v.clip.xy, v.clip.zw);
    let lo = v.rect.xy;
    let hi = v.rect.xy + v.rect.zw;
    if kind > 4.5 {
        let dims = vec2<i32>(textureDimensions(glyph_atlas));
        let at = clamp(vec2<i32>((p - lo) / max(hi-lo, vec2<f32>(0.0001)) * vec2<f32>(dims)),vec2<i32>(0),dims-1);
        return textureLoad(glyph_atlas, at, 0) * clip;
    }
    let texel = vec2<i32>(floor(p - lo)) + vec2<i32>(v.extra.xy);
    let paint = vec4<f32>(v.color.rgb * v.color.a, v.color.a);
    if kind < BORDER - 0.5 {
        return paint * rounded_coverage(p, lo, hi, v.extra.x) * clip;
    }
    if kind < GLYPH - 0.5 {
        let w = v.extra.y;
        let outer = rounded_coverage(p, lo, hi, v.extra.x);
        var inner = 0.0;
        if all(hi - lo > vec2<f32>(2.0 * w)) {
            inner = rounded_coverage(p, lo + vec2<f32>(w), hi - vec2<f32>(w), max(v.extra.x - w, 0.0));
        }
        return paint * max(outer - inner, 0.0) * clip;
    }
    if kind < COLOR_GLYPH - 0.5 {
        return paint * textureLoad(glyph_atlas, texel, 0).a * clip;
    }
    if kind < PATH - 0.5 {
        return textureLoad(glyph_atlas, texel, 0) * v.color.a * clip;
    }
    return paint * textureLoad(path_atlas, texel, 0).r * clip;
}
