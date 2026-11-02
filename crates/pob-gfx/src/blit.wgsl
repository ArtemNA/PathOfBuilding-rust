// Second pass (SimpleGraphic's "Scaling" pass, SG-DLL-FACTS §2): copies the y-up frame texture to the window
// surface, flipping it back; opaque output.

@group(0) @binding(0) var frame: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen.
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let size = textureDimensions(frame);
    let x = u32(p.x);
    let y = size.y - 1u - u32(p.y);
    return vec4<f32>(textureLoad(frame, vec2<u32>(x, y), 0).rgb, 1.0);
}
