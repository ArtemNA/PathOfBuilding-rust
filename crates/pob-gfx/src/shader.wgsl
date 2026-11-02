// Main pass of the SimpleGraphic model (SG-DLL-FACTS §2): colour = texture * tint; fragments outside the command's
// viewport rectangle are discarded, not scissored.
//
// The frame is rendered **y-up** (row 0 = bottom of the screen), as the original's GL ES-on-ANGLE does, so that the
// rasteriser's tie rules match it: pixel centres exactly on a left or *bottom* screen edge are inside, on a top or
// right edge outside (SG oracle scene `fractional`). The blit pass / readback flips the image back.

struct Screen {
    // physical width, height
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) clip: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    // Screen y grows downwards; NDC y up -> screen top lands on the last texture row (y-up frame).
    let ndc = v.pos / screen.size.xy * 2.0 - 1.0;
    o.pos = vec4<f32>(ndc.x, ndc.y, 0.0, 1.0);
    o.uv = v.uv;
    o.tint = v.tint;
    o.clip = v.clip;
    return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    // Sample before the discard: textureSample needs uniform control flow.
    let c = textureSample(tex, samp, i.uv) * i.tint;
    let x = i.pos.x;
    let y = screen.size.y - i.pos.y; // screen-space y of the pixel centre
    // SG: discard if x < x0 || x >= x1 in x, and (NDC y flipped) y <= y0 || y > y1.
    if (x < i.clip.x || x >= i.clip.z || y <= i.clip.y || y > i.clip.w) {
        discard;
    }
    return c;
}
