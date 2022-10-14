@group(0) @binding(0) var input_texture : texture_2d<f32>;
@group(0) @binding(1) var output_texture : texture_storage_2d<rgba8unorm, write>;

@compute
@workgroup_size(16, 16)
fn main(
  @builtin(global_invocation_id) global_id : vec3<u32>,
) {
    let dimensions = textureDimensions(input_texture);
    if(i32(global_id.x) >= dimensions.x || i32(global_id.y) >= dimensions.y) {
        return;
    }

    let target_position = vec2<i32>(dimensions.x - i32(global_id.x) - 1, i32(global_id.y));
    let color = textureLoad(input_texture, vec2<i32>(global_id.xy), 0);

    textureStore(output_texture, vec2<i32>(target_position), color);
}