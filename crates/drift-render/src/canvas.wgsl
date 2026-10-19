// Drift canvas shaders.
//
// All positions arrive in physical pixels relative to the canvas centre (y-down), already made
// camera-relative in f64 on the CPU. Strokes are signed-distance fields with 1 px anti-aliasing.
// Restricted to WebGL2-compatible features: uniforms and instanced vertex buffers only.

struct Globals {
    viewport: vec4<f32>,      // width px, height px, srgb target, unused
    grid_minor: vec4<f32>,    // spacing px, phase x, phase y, alpha
    grid_major: vec4<f32>,    // spacing px, phase x, phase y, visibility (0 or 1)
    axes: vec4<f32>,          // origin x px, origin y px, show axes, unused
    background: vec4<f32>,
    minor_color: vec4<f32>,
    major_color: vec4<f32>,
    axis_x_color: vec4<f32>,
    axis_y_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

// A picture's texture (premultiplied sRGB) and how it is sampled.
@group(1) @binding(0) var image_texture: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

const TAU: f32 = 6.283185307179586;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(2.0 * p.x / g.viewport.x, -2.0 * p.y / g.viewport.y, 0.0, 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// Premultiplied output colour. egui prefers a gamma-space framebuffer; linearise if not.
fn shade(color: vec4<f32>, coverage: f32) -> vec4<f32> {
    var rgb = color.rgb;
    if (g.viewport.z > 0.5) {
        rgb = srgb_to_linear(rgb);
    }
    let a = color.a * coverage;
    return vec4<f32>(rgb * a, a);
}

// Triangle-strip quad corners: (-1,-1), (1,-1), (-1,1), (1,1).
fn quad_corner(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u)) * 2.0 - vec2<f32>(1.0);
}

fn interval_coverage(t: f32, a: f32, b: f32) -> f32 {
    return clamp(min(t - a, b - t) + 0.5, 0.0, 1.0);
}

// Dash pattern [dash, gap, dot, phase] evaluated at arclength `s` (pixels). A negative `dot`
// means two dots per period (phantom lines).
fn dash_coverage(s: f32, pattern: vec4<f32>) -> f32 {
    let dash = pattern.x;
    if (dash <= 0.0) {
        return 1.0;
    }
    let gap = pattern.y;
    let dot = abs(pattern.z);
    let dots = select(1.0, 2.0, pattern.z < 0.0);
    let period = select(dash + gap, dash + (dots + 1.0) * gap + dots * dot, dot > 0.0);
    let u = s + pattern.w;
    let t = u - period * floor(u / period);
    var c = max(interval_coverage(t, 0.0, dash), interval_coverage(t, period, period + dash));
    if (dot > 0.0) {
        c = max(c, interval_coverage(t, dash + gap, dash + gap + dot));
        if (dots > 1.5) {
            c = max(c, interval_coverage(t, dash + 2.0 * gap + dot, dash + 2.0 * gap + 2.0 * dot));
        }
    }
    return c;
}

// ---------------------------------------------------------------------------------------------
// Grid (full-screen triangle)

struct GridOut {
    @builtin(position) position: vec4<f32>,
    @location(0) px: vec2<f32>,
};

@vertex
fn vs_grid(@builtin(vertex_index) i: u32) -> GridOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    let clip = uv * 2.0 - vec2<f32>(1.0);
    var out: GridOut;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.px = vec2<f32>(clip.x * 0.5 * g.viewport.x, -clip.y * 0.5 * g.viewport.y);
    return out;
}

fn grid_line(coord: f32, phase: f32, spacing: f32, half_width: f32) -> f32 {
    let t = coord - phase;
    let d = abs(t - spacing * round(t / spacing));
    return clamp(half_width + 0.5 - d, 0.0, 1.0);
}

@fragment
fn fs_grid(in: GridOut) -> @location(0) vec4<f32> {
    let p = in.px;
    var color = g.background.rgb;

    let minor = max(
        grid_line(p.x, g.grid_minor.y, g.grid_minor.x, 0.5),
        grid_line(p.y, g.grid_minor.z, g.grid_minor.x, 0.5),
    ) * g.grid_minor.w;
    color = mix(color, g.minor_color.rgb, minor * g.minor_color.a);

    let major = max(
        grid_line(p.x, g.grid_major.y, g.grid_major.x, 0.5),
        grid_line(p.y, g.grid_major.z, g.grid_major.x, 0.5),
    ) * g.grid_major.w;
    color = mix(color, g.major_color.rgb, major * g.major_color.a);

    if (g.axes.z > 0.5) {
        let x_axis = clamp(1.25 - abs(p.y - g.axes.y), 0.0, 1.0);
        color = mix(color, g.axis_x_color.rgb, x_axis * g.axis_x_color.a);
        let y_axis = clamp(1.25 - abs(p.x - g.axes.x), 0.0, 1.0);
        color = mix(color, g.axis_y_color.rgb, y_axis * g.axis_y_color.a);
    }

    return shade(vec4<f32>(color, 1.0), 1.0);
}

// ---------------------------------------------------------------------------------------------
// Filled triangles

struct FillIn {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct FillOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_fill(in: FillIn) -> FillOut {
    var out: FillOut;
    out.position = to_clip(in.pos);
    out.color = in.color;
    return out;
}

@fragment
fn fs_fill(in: FillOut) -> @location(0) vec4<f32> {
    return shade(in.color, 1.0);
}

// ---------------------------------------------------------------------------------------------
// Pictures (textured triangles)

struct ImageIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
};

struct ImageOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex
fn vs_image(in: ImageIn) -> ImageOut {
    var out: ImageOut;
    out.position = to_clip(in.pos);
    out.uv = in.uv;
    out.alpha = in.alpha;
    return out;
}

@fragment
fn fs_image(in: ImageOut) -> @location(0) vec4<f32> {
    // Texels are premultiplied (so filtering doesn't bleed see-through colours): undo that
    // to linearise, then premultiply again with the picture's own opacity.
    let c = textureSample(image_texture, image_sampler, in.uv);
    var rgb = c.rgb / max(c.a, 1e-6);
    if (g.viewport.z > 0.5) {
        rgb = srgb_to_linear(rgb);
    }
    let a = c.a * in.alpha;
    return vec4<f32>(rgb * a, a);
}

// ---------------------------------------------------------------------------------------------
// Line segments

struct SegmentIn {
    @location(0) p0: vec2<f32>,
    @location(1) p1: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) half_width: f32,
    @location(4) pattern: vec4<f32>,
};

struct SegmentOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,      // along, across (pixels)
    @location(1) color: vec4<f32>,
    @location(2) shape: vec2<f32>,      // length, half width
    @location(3) pattern: vec4<f32>,
};

@vertex
fn vs_segment(@builtin(vertex_index) vi: u32, inst: SegmentIn) -> SegmentOut {
    let d = inst.p1 - inst.p0;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 1e-6) {
        dir = d / len;
    }
    let normal = vec2<f32>(-dir.y, dir.x);
    let extent = inst.half_width + 1.0;
    let corner = quad_corner(vi);
    let along = select(-extent, len + extent, corner.x > 0.0);
    let across = corner.y * extent;

    var out: SegmentOut;
    out.position = to_clip(inst.p0 + dir * along + normal * across);
    out.local = vec2<f32>(along, across);
    out.color = inst.color;
    out.shape = vec2<f32>(len, inst.half_width);
    out.pattern = inst.pattern;
    return out;
}

@fragment
fn fs_segment(in: SegmentOut) -> @location(0) vec4<f32> {
    let len = in.shape.x;
    let half_width = in.shape.y;
    let beyond = max(max(-in.local.x, in.local.x - len), 0.0);
    let d = length(vec2<f32>(beyond, in.local.y));
    let coverage = clamp(half_width + 0.5 - d, 0.0, 1.0)
        * dash_coverage(clamp(in.local.x, 0.0, len), in.pattern);
    if (coverage <= 0.0) {
        discard;
    }
    return shade(in.color, coverage);
}

// ---------------------------------------------------------------------------------------------
// Circles, arcs and points (radius 0)

struct ArcIn {
    @location(0) center: vec2<f32>,
    @location(1) params: vec4<f32>,     // radius, half width, start angle, sweep
    @location(2) color: vec4<f32>,
    @location(3) pattern: vec4<f32>,
};

struct ArcOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,      // offset from centre (pixels, y-down)
    @location(1) params: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) pattern: vec4<f32>,
};

@vertex
fn vs_arc(@builtin(vertex_index) vi: u32, inst: ArcIn) -> ArcOut {
    let extent = inst.params.x + inst.params.y + 1.0;
    let local = quad_corner(vi) * extent;
    var out: ArcOut;
    out.position = to_clip(inst.center + local);
    out.local = local;
    out.params = inst.params;
    out.color = inst.color;
    out.pattern = inst.pattern;
    return out;
}

@fragment
fn fs_arc(in: ArcOut) -> @location(0) vec4<f32> {
    let p = in.local;
    let radius = in.params.x;
    let half_width = in.params.y;
    let start = in.params.z;
    let sweep = in.params.w;

    // Angle in world orientation (y-up), relative to the arc start, in [0, TAU).
    let angle = atan2(-p.y, p.x) - start;
    let rel = angle - TAU * floor(angle / TAU);

    var d: f32;
    var s: f32;
    if (rel <= sweep) {
        d = abs(length(p) - radius);
        s = rel * radius;
    } else {
        // Outside the sweep: distance to the nearer end point (round caps).
        let end = start + sweep;
        let e0 = radius * vec2<f32>(cos(start), -sin(start));
        let e1 = radius * vec2<f32>(cos(end), -sin(end));
        let d0 = length(p - e0);
        let d1 = length(p - e1);
        d = min(d0, d1);
        s = select(sweep * radius, 0.0, d0 < d1);
    }

    let coverage = clamp(half_width + 0.5 - d, 0.0, 1.0) * dash_coverage(s, in.pattern);
    if (coverage <= 0.0) {
        discard;
    }
    return shade(in.color, coverage);
}
