[[group(0), binding(0)]] var input_texture : texture_2d<f32>;
[[group(0), binding(1)]] var output_texture : texture_storage_2d<rgba8unorm, write>;

[[stage(compute), workgroup_size(16, 16)]]
fn main(
  [[builtin(global_invocation_id)]] global_id : vec3<u32>,
) {
    let dimensions = textureDimensions(input_texture);
    if(i32(global_id.x) >= dimensions.x || i32(global_id.y) >= dimensions.y) {
        return;
    }

    let color = textureLoad(input_texture, vec2<i32>(global_id.xy), 0);
    let gray = 0.299 * color.r + 0.587 * color.g + 0.114 * color.b;

    textureStore(output_texture, vec2<i32>(global_id.xy), vec4<f32>(gray, gray, gray, color.a));
}
