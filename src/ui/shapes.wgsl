// Shapes styled after the Sonori icon: a frosted white glass tile with
// polished silver capsule bars. Each instance is a quad; the fragment shader
// draws one signed-distance shape in it, chosen by params.x (see shapes.rs).
// Colours are sRGB values on a non-sRGB surface.

struct Globals {
    screen: vec2<f32>,
    time: f32,
    alpha: f32,
};

@group(0) @binding(0) var<uniform> g: Globals;

struct ShapeIn {
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) params: vec4<f32>,
    @location(3) extra: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) extra: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, s: ShapeIn) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let px = s.rect.xy + corners[vi] * s.rect.zw;
    var out: VsOut;
    out.pos = vec4<f32>(px.x / g.screen.x * 2.0 - 1.0, 1.0 - px.y / g.screen.y * 2.0, 0.0, 1.0);
    out.half_size = s.rect.zw * 0.5;
    out.local = px - (s.rect.xy + out.half_size);
    out.color = s.color;
    out.params = s.params;
    out.extra = s.extra;
    return out;
}

fn rgb(hex: u32) -> vec3<f32> {
    return vec3<f32>(f32((hex >> 16u) & 255u), f32((hex >> 8u) & 255u), f32(hex & 255u)) / 255.0;
}

// Coverage of a pixel by a shape at signed distance d (pixels).
fn cover(d: f32) -> f32 {
    return clamp(0.5 - d, 0.0, 1.0);
}

fn rounded_box(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Vertical capsule filling `half_size` (its width sets the cap radius).
fn capsule(p: vec2<f32>, half_size: vec2<f32>) -> f32 {
    let r = half_size.x;
    return length(vec2<f32>(p.x, max(abs(p.y) - (half_size.y - r), 0.0))) - r;
}

fn premul(c: vec4<f32>, coverage: f32) -> vec4<f32> {
    let a = c.a * coverage;
    return vec4<f32>(c.rgb * a, a);
}

// Premultiplied `src` over `dst`.
fn over(src: vec4<f32>, dst: vec4<f32>) -> vec4<f32> {
    return src + dst * (1.0 - src.a);
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// The icon's horizontal silver gradient across a bar, u in [0, 1].
fn silver(u: f32) -> vec3<f32> {
    if (u < 0.18) { return mix(rgb(0x9aa5b3u), rgb(0xe4eaf1u), u / 0.18); }
    if (u < 0.36) { return mix(rgb(0xe4eaf1u), rgb(0xffffffu), (u - 0.18) / 0.18); }
    if (u < 0.64) { return mix(rgb(0xffffffu), rgb(0xd5dce5u), (u - 0.36) / 0.28); }
    return mix(rgb(0xd5dce5u), rgb(0x8c98a8u), (u - 0.64) / 0.36);
}

// The icon's vertical reflection along a bar, v in [0, 1] from the top.
fn reflection(v: f32) -> vec4<f32> {
    let white = vec3<f32>(1.0);
    if (v < 0.42) { return mix(vec4<f32>(white, 0.85), vec4<f32>(white, 0.05), v / 0.42); }
    if (v < 0.48) { return mix(vec4<f32>(white, 0.05), vec4<f32>(rgb(0x6a778bu), 0.20), (v - 0.42) / 0.06); }
    if (v < 0.64) { return mix(vec4<f32>(rgb(0x6a778bu), 0.20), vec4<f32>(white, 0.20), (v - 0.48) / 0.16); }
    return mix(vec4<f32>(white, 0.20), vec4<f32>(rgb(0x5b697eu), 0.28), (v - 0.64) / 0.36);
}

// The icon's glass fill, diagonal from the top-left, u in [0, 1].
fn glass_fill(u: f32) -> vec3<f32> {
    if (u < 0.45) { return mix(rgb(0xffffffu), rgb(0xedf3f9u), u / 0.45); }
    return mix(rgb(0xedf3f9u), rgb(0xd1dce8u), (u - 0.45) / 0.55);
}

fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    return length(pa - ba * clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0));
}

// Signed distance to a closed polygon of `n` (at most 8) vertices.
fn polygon(p: vec2<f32>, verts: array<vec2<f32>, 8>, n: i32) -> f32 {
    var v = verts;
    var d = dot(p - v[0], p - v[0]);
    var s = 1.0;
    var j = n - 1;
    for (var i = 0; i < n; i = i + 1) {
        let e = v[j] - v[i];
        let w = p - v[i];
        let b = w - e * clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
        d = min(d, dot(b, b));
        let c = vec3<bool>(p.y >= v[i].y, p.y < v[j].y, e.x * w.y > e.y * w.x);
        if (all(c) || !any(c)) {
            s = -s;
        }
        j = i;
    }
    return s * sqrt(d);
}

fn star(p: vec2<f32>, c: vec2<f32>, r: f32) -> f32 {
    let q = 0.28 * r;
    let verts = array<vec2<f32>, 8>(
        c + vec2<f32>(0.0, -r), c + vec2<f32>(q, -q), c + vec2<f32>(r, 0.0), c + vec2<f32>(q, q),
        c + vec2<f32>(0.0, r), c + vec2<f32>(-q, q), c + vec2<f32>(-r, 0.0), c + vec2<f32>(-q, -q),
    );
    return polygon(p, verts, 8);
}

// Coverage of icon `id`, drawn on an 18-unit grid that fills the quad.
fn icon(id: i32, p: vec2<f32>, half_size: vec2<f32>, angle: f32) -> f32 {
    let k = half_size.x / 9.0;
    var c = p / k;
    if (angle != 0.0) {
        let cs = cos(angle);
        let sn = sin(angle);
        c = vec2<f32>(cs * c.x + sn * c.y, -sn * c.x + cs * c.y);
    }
    let s = c + vec2<f32>(9.0);
    var d = 1e4;
    switch id {
        // Globe: language.
        case 0: {
            d = abs(length(c) - 6.5) - 0.75;
            d = min(d, segment(s, vec2<f32>(2.5, 9.0), vec2<f32>(15.5, 9.0)) - 0.65);
            d = min(d, abs((length(c / vec2<f32>(2.6, 6.5)) - 1.0) * 2.6) - 0.65);
        }
        // Sparkles: Magic Mode.
        case 1: {
            d = min(star(s, vec2<f32>(8.5, 7.0), 5.2), star(s, vec2<f32>(14.0, 13.6), 2.4));
        }
        // Page: output.
        case 2: {
            let page = array<vec2<f32>, 8>(
                vec2<f32>(4.0, 3.0), vec2<f32>(11.0, 3.0), vec2<f32>(14.0, 6.0), vec2<f32>(14.0, 15.0),
                vec2<f32>(4.0, 15.0), vec2<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0),
            );
            d = abs(polygon(s, page, 5)) - 0.7;
            d = min(d, segment(s, vec2<f32>(11.0, 3.0), vec2<f32>(11.0, 6.0)) - 0.7);
            d = min(d, segment(s, vec2<f32>(11.0, 6.0), vec2<f32>(14.0, 6.0)) - 0.7);
            d = min(d, segment(s, vec2<f32>(7.0, 10.0), vec2<f32>(11.0, 10.0)) - 0.7);
            d = min(d, segment(s, vec2<f32>(7.0, 12.5), vec2<f32>(11.0, 12.5)) - 0.7);
        }
        // Three rising bars: quality.
        case 3: {
            d = segment(s, vec2<f32>(4.2, 10.2), vec2<f32>(4.2, 13.8)) - 1.2;
            d = min(d, segment(s, vec2<f32>(9.0, 6.2), vec2<f32>(9.0, 13.8)) - 1.2);
            d = min(d, segment(s, vec2<f32>(13.8, 3.2), vec2<f32>(13.8, 13.8)) - 1.2);
        }
        // Pointer with a ring: display.
        case 4: {
            let arrow = array<vec2<f32>, 8>(
                vec2<f32>(3.0, 3.0), vec2<f32>(3.0, 12.0), vec2<f32>(5.7, 9.6), vec2<f32>(7.6, 14.0),
                vec2<f32>(9.4, 13.2), vec2<f32>(7.6, 8.9), vec2<f32>(11.0, 8.9), vec2<f32>(0.0),
            );
            d = polygon(s, arrow, 7);
            d = min(d, abs(length(s - vec2<f32>(13.2, 12.6)) - 2.8) - 0.7);
        }
        // Gear: settings.
        case 5: {
            d = abs(length(c) - 2.3) - 0.75;
            for (var i = 0; i < 8; i = i + 1) {
                let a = f32(i) * 0.785398;
                let dir = vec2<f32>(cos(a), sin(a));
                d = min(d, segment(c, dir * 4.8, dir * 7.0) - 0.75);
            }
        }
        // Chevron pointing right; `angle` turns it.
        case 6: {
            d = segment(s, vec2<f32>(6.6, 3.8), vec2<f32>(11.6, 9.0)) - 1.45;
            d = min(d, segment(s, vec2<f32>(11.6, 9.0), vec2<f32>(6.6, 14.2)) - 1.45);
        }
        // Tick.
        case 7: {
            d = segment(s, vec2<f32>(3.9, 9.6), vec2<f32>(7.2, 12.9)) - 1.15;
            d = min(d, segment(s, vec2<f32>(7.2, 12.9), vec2<f32>(14.1, 5.1)) - 1.15);
        }
        // Microphone: a capsule on a stand.
        case 8: {
            d = abs(rounded_box(s - vec2<f32>(9.0, 6.5), vec2<f32>(2.5, 4.5), 2.5)) - 0.75;
            var arc = abs(length(s - vec2<f32>(9.0, 8.5)) - 4.5) - 0.75;
            if (s.y < 8.5) {
                arc = min(length(s - vec2<f32>(4.5, 8.5)), length(s - vec2<f32>(13.5, 8.5))) - 0.75;
            }
            d = min(d, arc);
            d = min(d, segment(s, vec2<f32>(9.0, 13.0), vec2<f32>(9.0, 16.0)) - 0.75);
        }
        // Speech bubble with two lines of text: words.
        case 9: {
            d = abs(rounded_box(s - vec2<f32>(9.0, 7.8), vec2<f32>(6.3, 4.8), 3.0)) - 0.75;
            d = min(d, segment(s, vec2<f32>(6.2, 12.6), vec2<f32>(5.0, 15.8)) - 0.75);
            d = min(d, segment(s, vec2<f32>(5.0, 15.8), vec2<f32>(9.6, 12.6)) - 0.75);
            d = min(d, segment(s, vec2<f32>(6.2, 6.7), vec2<f32>(11.8, 6.7)) - 0.65);
            d = min(d, segment(s, vec2<f32>(6.2, 9.3), vec2<f32>(9.8, 9.3)) - 0.65);
        }
        // Cross: close and remove.
        case 10: {
            d = segment(s, vec2<f32>(3.0, 3.0), vec2<f32>(15.0, 15.0)) - 1.3;
            d = min(d, segment(s, vec2<f32>(15.0, 3.0), vec2<f32>(3.0, 15.0)) - 1.3);
        }
        default: {}
    }
    return cover(d * k);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let kind = i32(in.params.x + 0.5);
    let p = in.local;
    var out = vec4<f32>(0.0);

    switch kind {
        // Rounded-rectangle outline: params.y corner radius, params.z width, extra color.
        case 0: {
            let d = rounded_box(p, in.half_size - vec2<f32>(0.5), in.params.y);
            let bw = in.params.z;
            out = premul(in.extra, cover(abs(d + bw * 0.5) - bw * 0.5));
        }
        // Frosted glass tile: params.y corner radius, params.z shadow margin,
        // params.w edge glow; color.a opacity, extra.x fill alpha.
        case 1: {
            let margin = in.params.z;
            let hs = in.half_size - vec2<f32>(margin);
            let corner = in.params.y;
            let d = rounded_box(p, hs, corner);
            let inside = cover(d);
            let n = p / hs;

            // Cool-grey shadow, so the tile stands off white pages.
            let sigma = max(margin * 0.45, 1.0);
            let ds = rounded_box(p - vec2<f32>(0.0, sigma * 0.5), hs, corner);
            let shadow = 0.22 * exp(-pow(max(ds, 0.0) / sigma, 2.0)) * (1.0 - inside);
            var acc = premul(vec4<f32>(rgb(0x718096u), 1.0), shadow);

            let diag = clamp(dot(n, vec2<f32>(0.5, 0.5)) * 0.5 + 0.5, 0.0, 1.0);
            acc = over(premul(vec4<f32>(glass_fill(diag), in.extra.x), inside), acc);

            // Broad curved reflection over the upper surface.
            let nx = n.x * 0.5 + 0.5;
            let boundary = 0.05 - 0.75 * nx + 0.35 * nx * nx;
            let above = smoothstep(boundary + 0.05, boundary - 0.05, n.y);
            let sheen = mix(0.55, 0.12, clamp((n.y + 1.0) / (boundary + 1.001), 0.0, 1.0));
            acc = over(premul(vec4<f32>(1.0, 1.0, 1.0, sheen), inside * above), acc);

            // Restrained reflection along the inside of the lower edge.
            let caustic = cover(abs(d + 0.09 * hs.y) - 0.8)
                * smoothstep(0.5, 0.85, n.y) * (1.0 - n.x * n.x) * 0.7;
            acc = over(premul(vec4<f32>(1.0, 1.0, 1.0, 1.0), caustic), acc);

            // Inner reflective edge: bright at two corners, blue-grey between.
            var edge = vec4<f32>(rgb(0x90a8c3u), 0.30);
            if (diag < 0.35) {
                edge = mix(vec4<f32>(1.0, 1.0, 1.0, 1.0), vec4<f32>(1.0, 1.0, 1.0, 0.2), diag / 0.35);
            } else if (diag > 0.65) {
                edge = mix(vec4<f32>(rgb(0x90a8c3u), 0.30), vec4<f32>(1.0, 1.0, 1.0, 0.95), (diag - 0.65) / 0.35);
            }
            acc = over(premul(edge, cover(abs(d + 2.4) - 0.75)), acc);

            // Thin outer rim: white at the top, cool grey at the bottom.
            let rim = cover(d) - cover(d + 1.0);
            let rim_color = mix(vec4<f32>(1.0, 1.0, 1.0, 1.0), vec4<f32>(rgb(0xabb5c3u), 0.65), n.y * 0.5 + 0.5);
            acc = over(premul(rim_color, rim), acc);

            // Hairline outside the rim, so the edge holds on white pages.
            let hairline = clamp(1.5 - d, 0.0, 1.0) - clamp(0.5 - d, 0.0, 1.0);
            acc = over(premul(vec4<f32>(rgb(0x90a8c3u), 0.45), hairline), acc);

            // Brief white glow around the tile when a transcript lands.
            let glow = in.params.w * 0.8 * exp(-max(d, 0.0) / 4.0) * (1.0 - inside);
            acc = over(premul(vec4<f32>(1.0, 1.0, 1.0, 1.0), glow), acc);

            out = acc * in.color.a;
        }
        // Polished silver capsule bar: params.z shadow margin; color.a opacity.
        case 2: {
            let margin = in.params.z;
            let hs = in.half_size - vec2<f32>(margin);
            let d = capsule(p, hs);
            let inside = cover(d);
            let ds = capsule(p - vec2<f32>(0.0, margin * 0.4), hs);
            let shadow = 0.25 * exp(-pow(max(ds, 0.0) / max(margin * 0.5, 0.8), 2.0));
            var acc = premul(vec4<f32>(rgb(0x69798fu), 1.0), shadow * (1.0 - inside));
            let u = clamp(p.x / hs.x * 0.5 + 0.5, 0.0, 1.0);
            acc = over(premul(vec4<f32>(silver(u), 1.0), inside), acc);
            let v = clamp(p.y / hs.y * 0.5 + 0.5, 0.0, 1.0);
            acc = over(premul(reflection(v), inside), acc);
            acc = over(premul(vec4<f32>(rgb(0x8e9aaau), 0.25), cover(abs(d + 0.4) - 0.4)), acc);
            out = acc * in.color.a;
        }
        // Menu control surface: params.y corner radius, params.z style, params.w
        // shadow margin. Style 0 fills with color (straight alpha); the others
        // use the icon palette and take color.a as opacity.
        case 3: {
            let margin = in.params.w;
            let hs = in.half_size - vec2<f32>(margin);
            let d = rounded_box(p, hs, in.params.y);
            let inside = cover(d);
            let style = i32(in.params.z + 0.5);
            let v = clamp(p.y / hs.y * 0.5 + 0.5, 0.0, 1.0);
            var acc = vec4<f32>(0.0);
            var opacity = in.color.a;
            if (style == 0) {
                acc = premul(in.color, inside);
                opacity = 1.0;
            } else if (style == 1) {
                // Raised: white to #eef3f8, soft drop shadow, cool ring.
                let ds = rounded_box(p - vec2<f32>(0.0, 1.0), hs, in.params.y);
                let shadow = 0.2 * exp(-pow(max(ds, 0.0) / max(margin * 0.6, 0.8), 2.0)) * (1.0 - inside);
                acc = premul(vec4<f32>(rgb(0x3b4a60u), 1.0), shadow);
                acc = over(premul(vec4<f32>(mix(vec3<f32>(1.0), rgb(0xeef3f8u), v), 1.0), inside), acc);
                acc = over(premul(vec4<f32>(rgb(0x90a8c3u), 0.42), cover(abs(d + 0.5) - 0.5)), acc);
            } else if (style == 2 || style == 3) {
                // Sunk track (2) or slate track (3), shaded at the top edge.
                var fill = vec4<f32>(rgb(0x90a8c3u), 0.2);
                if (style == 3) {
                    fill = vec4<f32>(rgb(0x3b4a60u), 1.0);
                }
                acc = premul(fill, inside);
                let inset = inside * exp(-(p.y + hs.y) / 2.2) * 0.22;
                acc = over(premul(vec4<f32>(rgb(0x1e2836u), 1.0), inset), acc);
                acc = over(premul(vec4<f32>(rgb(0x90a8c3u), 0.32), cover(abs(d + 0.5) - 0.5)), acc);
            } else {
                // Separator: a hairline that fades out towards both ends.
                let a = 0.6 * (1.0 - pow(abs(p.x) / hs.x, 2.0));
                acc = premul(vec4<f32>(rgb(0x90a8c3u), a), inside);
            }
            out = acc * opacity;
        }
        // Line icon on an 18-unit grid: params.y icon, params.w rotation; color.
        case 4: {
            out = premul(in.color, icon(i32(in.params.y + 0.5), p, in.half_size, in.params.w));
        }
        default: {}
    }

    out = out * g.alpha;
    // Dither soft gradients, so they do not band.
    let noise = (hash(in.pos.xy) - 0.5) / 255.0;
    return clamp(out + vec4<f32>(noise) * step(0.002, out.a), vec4<f32>(0.0), vec4<f32>(1.0));
}
