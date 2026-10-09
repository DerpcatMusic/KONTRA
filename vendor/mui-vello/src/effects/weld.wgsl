// MUI analytic material welding for native wgpu. The earlier WebGL/GLSL
// reference is not evidence that this WGSL has compiled or rendered.
// Uniform ABI: 21 vec4<f32>, 336 bytes, all offsets multiples of 16.
// Supported geometry: one to three axis-aligned rounded rectangles. The
// cos/sin lane is always (1, 0): no source sets a rotation (mui-weld analytic.rs).
struct Source {
    geom: vec4<f32>,   // centre.xy, half-size.xy
    params: vec4<f32>, // radius, inside-border width, cos(angle), sin(angle)
    fill0: vec4<f32>,  // Oklab + alpha
    fill1: vec4<f32>,
    edge: vec4<f32>,
    grad: vec4<f32>,   // start in source-local coordinates, projection vector
}
struct Params {
    viewport: vec4<f32>, // texture size.xy, logical domain size.xy
    cfg: vec4<f32>,      // reach, material blend, morph, source count
    policy: vec4<f32>,   // fill policy, border policy, crisp edge count (0=organic), fixed AA
    sources: array<Source, 3>,
}
@group(0) @binding(0) var<uniform> u: Params;
struct Edge { geom:vec4<f32>, directions:vec4<f32>, info:vec4<f32> }
struct Boundary { edges:array<Edge,128> }
@group(0) @binding(1) var<uniform> boundary:Boundary;
fn cross2(a:vec2<f32>,b:vec2<f32>)->f32 { return a.x*b.y-a.y*b.x; }
fn nearest_edge(e:Edge,p:vec2<f32>)->vec2<f32> {
    if e.info.x<0.5 {
        let v=e.geom.zw-e.geom.xy;
        return e.geom.xy+v*clamp(dot(p-e.geom.xy,v)/max(dot(v,v),1e-20),0.,1.);
    }
    let v=p-e.geom.xy;let length_v=length(v);
    let direction=v/max(length_v,1e-20);
    if length_v>1e-10 && cross2(e.directions.xy,direction)>=-1e-6 && cross2(direction,e.directions.zw)>=-1e-6 {
        return e.geom.xy+direction*e.geom.z;
    }
    let a=e.geom.xy+e.directions.xy*e.geom.z;
    let b=e.geom.xy+e.directions.zw*e.geom.z;
    return select(b,a,distance(p,a)<=distance(p,b));
}
fn nearest_boundary(p:vec2<f32>)->vec2<f32> {
    var best=1e30;var point=p;
    for(var i=0u;i<min(u32(u.policy.z),128u);i=i+1u){
        let q=nearest_edge(boundary.edges[i],p);let d=dot(q-p,q-p);
        if d<best{best=d;point=q;}
    }
    return point;
}
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) point: vec2<f32>,
}
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> Vertex {
    let corners = array<vec2<f32>, 6>(
        vec2(0., 0.), vec2(1., 0.), vec2(0., 1.),
        vec2(0., 1.), vec2(1., 0.), vec2(1., 1.));
    let p = corners[i] * u.viewport.zw;
    var o: Vertex;
    o.position = vec4(p.x / u.viewport.z * 2. - 1.,
                      1. - p.y / u.viewport.w * 2., 0., 1.);
    o.point = p;
    return o;
}
fn local_point(s: Source, p: vec2<f32>) -> vec2<f32> {
    let q = p - s.geom.xy;
    return vec2(s.params.z*q.x + s.params.w*q.y,
               -s.params.w*q.x + s.params.z*q.y);
}
fn rect_distance(s: Source, p: vec2<f32>) -> f32 {
    let r = min(s.params.x, min(s.geom.z, s.geom.w));
    let q = abs(local_point(s, p)) - s.geom.zw + vec2(r);
    return length(max(q, vec2(0.))) + min(max(q.x, q.y), 0.) - r;
}
fn weights_at(d: vec3<f32>, k: f32) -> vec3<f32> {
    let lo = min(d.x, min(d.y, d.z));
    if k <= 0.000001 {
        let ties = select(vec3(0.), vec3(1.), d == vec3(lo));
        return ties / dot(ties, vec3(1.));
    }
    let q = d - vec3(lo);
    let hi = max(q.x, max(q.y, q.z));
    let mid = max(min(q.x, q.y), min(max(q.x, q.y), q.z));
    var lambda = k;
    if mid < lambda { lambda = (k + mid)*0.5; }
    if hi < lambda { lambda = (k + mid + hi)/3.; }
    let w = max(vec3(lambda) - q, vec3(0.))/k;
    return w/dot(w, vec3(1.));
}
fn joined_distance(d: vec3<f32>, k: f32) -> f32 {
    let lo = min(d.x, min(d.y, d.z));
    let w = weights_at(d, k);
    return lo + dot(w, d - vec3(lo)) - k*0.5*(1. - dot(w, w));
}
fn gradient_lab(s: Source, p: vec2<f32>) -> vec4<f32> {
    let t = clamp(dot(local_point(s,p)-s.grad.xy, s.grad.zw), 0., 1.);
    let a = mix(s.fill0.a, s.fill1.a, t);
    let lab = mix(s.fill0.rgb*s.fill0.a, s.fill1.rgb*s.fill1.a, t)/max(a, 1e-12);
    return vec4(lab, a);
}
fn lab_linear(v: vec4<f32>) -> vec4<f32> {
    var lms = vec3(v.x + .3963377774*v.y + .2158037573*v.z,
                   v.x - .1055613458*v.y - .0638541728*v.z,
                   v.x - .0894841775*v.y - 1.2914855480*v.z);
    lms = lms*lms*lms;
    return vec4(4.0767416621*lms.x-3.3077115913*lms.y+.2309699292*lms.z,
                -1.2684380046*lms.x+2.6097574011*lms.y-.3413193965*lms.z,
                -.0041960863*lms.x-.7034186147*lms.y+1.7076147010*lms.z, v.a);
}
fn mixed_lab(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, w: vec3<f32>) -> vec4<f32> {
    let alpha = dot(vec3(a.a, b.a, c.a), w);
    return vec4((a.rgb*a.a*w.x+b.rgb*b.a*w.y+c.rgb*c.a*w.z)/max(alpha,1e-12),alpha);
}
fn premul(c: vec4<f32>) -> vec4<f32> { return vec4(c.rgb*c.a,c.a); }
fn over(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> { return a+b*(1.-a.a); }
fn coverage(d: f32, aa: f32) -> f32 { return clamp(.5-d/aa,0.,1.); }
fn encode_srgb(c0: vec3<f32>) -> vec3<f32> {
    let c = clamp(c0, vec3(0.), vec3(1.));
    return select(1.055*pow(c,vec3(1./2.4))-vec3(.055), 12.92*c, c<=vec3(.0031308));
}
// Returns LINEAR premultiplied RGBA. Host encoding is decided only at the end.
fn shade(p: vec2<f32>) -> vec4<f32> {
    let n = u32(u.cfg.w);
    var ds = vec3(rect_distance(u.sources[0],p), 1e6, 1e6);
    if n > 1u { ds.y = rect_distance(u.sources[1],p); }
    if n > 2u { ds.z = rect_distance(u.sources[2],p); }
    let t = u.cfg.z*u.cfg.z*(3.-2.*u.cfg.z);
    var d = joined_distance(ds,2.*u.cfg.x*t);
    var border_point=p;
    let hard_min=min(ds.x,min(ds.y,ds.z));
    let border_limit=max(u.sources[0].params.y,max(u.sources[1].params.y,u.sources[2].params.y));
    let logical_pixel=max(u.viewport.z/u.viewport.x,u.viewport.w/u.viewport.y);
    // Far inside a source is at least as far inside its union. Exact boundary
    // searches are only needed in the conservative border band, not every fill pixel.
    if u.policy.z>0. && hard_min>=-border_limit-2.*logical_pixel && hard_min<=2.*logical_pixel {
        border_point=nearest_boundary(p);
        d=select(distance(p,border_point),-distance(p,border_point),min(ds.x,min(ds.y,ds.z))<=0.);
    }
    // No derivative after divergent return. The fixed-AA test mode still
    // evaluates derivatives unconditionally for WebGPU uniformity validation.
    let derivative = length(vec2(dpdx(d),dpdy(d)));
    let pixel = max(u.viewport.z/u.viewport.x, u.viewport.w/u.viewport.y);
    let aa = select(select(max(derivative,pixel),pixel,u.policy.z>0.),u.policy.w,u.policy.w>0.);
    let a = coverage(d,aa);
    if a <= 0. { return vec4(0.); }
    var labs: array<vec4<f32>,3>;
    var borders: array<vec4<f32>,3>;
    var original = vec4(0.);
    var old_fill = vec4(0.);
    for(var i=0u; i<3u; i=i+1u) {
        labs[i] = select(gradient_lab(u.sources[i],p),vec4(0.),u.policy.x==2.);
        borders[i] = select(u.sources[i].edge,vec4(0.),u.policy.y==2.);
        if i >= n { labs[i]=vec4(0.); borders[i]=vec4(0.); continue; }
        let sa = coverage(ds[i],aa);
        let ring = max(0.,sa-coverage(ds[i]+u.sources[i].params.y,aa));
        let f = premul(lab_linear(labs[i]));
        let b = premul(lab_linear(borders[i]));
        original = over(over(b*(ring/max(sa,1e-12)),f)*sa,original);
        old_fill = over(f*sa,old_fill);
    }
    var result = original;
    if t > 0. {
        let w = weights_at(ds,u.cfg.y*t);
        result = vec4(0.);
        if u.policy.x == 0. { result=premul(lab_linear(mixed_lab(labs[0],labs[1],labs[2],w)))*a; }
        else if u.policy.x == 1. { result=old_fill; }
        if u.policy.y == 0. {
            var bw=w;
            if u.policy.z>0. {
                var bd=vec3(abs(rect_distance(u.sources[0],border_point)),1e6,1e6);
                if n>1u{bd.y=abs(rect_distance(u.sources[1],border_point));}
                if n>2u{bd.z=abs(rect_distance(u.sources[2],border_point));}
                bw=weights_at(bd,u.cfg.y*t);
            }
            let width = dot(vec3(u.sources[0].params.y,u.sources[1].params.y,u.sources[2].params.y),bw);
            let b = lab_linear(mixed_lab(borders[0],borders[1],borders[2],bw));
            let ring = max(0.,a-coverage(d+width,aa));
            result = result*(1.-b.a*ring/max(a,1e-12))+premul(b)*ring;
        } else if u.policy.y == 1. {
            for(var i=0u;i<3u;i=i+1u) {
                if i >= n { break; }
                let b = lab_linear(borders[i]);
                let ring = max(0.,coverage(ds[i],aa)-coverage(ds[i]+u.sources[i].params.y,aa));
                let ratio = min(1.,ring/max(a,1e-12));
                result = result*(1.-b.a*ratio)+premul(b)*ring;
            }
        }
        let base = coverage(min(ds.x,min(ds.y,ds.z)),aa);
        let added = clamp((a-base)/max(a,1e-12),0.,1.);
        result = mix(original,result,t+(1.-t)*added);
    }
    return result;
}
// Rgba8Unorm, already sRGB-encoded, premultiplied: Hybrid external texture.
@fragment fn fs_hybrid(vertex: Vertex) -> @location(0) vec4<f32> {
    let c = shade(vertex.point);
    return vec4(encode_srgb(c.rgb/max(c.a,1e-12))*c.a,c.a);
}
