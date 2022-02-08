[[group(0), binding(0)]] var samp: sampler;
[[group(1), binding(0)]] var input_texture : texture_2d<f32>;
[[group(1), binding(1)]] var output_texture : texture_storage_2d<rgba8unorm, write>;

[[stage(compute), workgroup_size(16, 16)]]
fn main(
  [[builtin(global_invocation_id)]] global_id : vec3<u32>,
) {
    let dimensions = textureDimensions(output_texture);
    if(i32(global_id.x) >= dimensions.x || i32(global_id.y) >= dimensions.y) {
        return;
    }

    let tex_coords = vec2<f32>(f32(global_id.x)/f32(dimensions.x), f32(global_id.y)/f32(dimensions.y));
    let color = textureSampleLevel(input_texture, samp, tex_coords, 0.0);

    textureStore(output_texture, vec2<i32>(global_id.xy), color);
}