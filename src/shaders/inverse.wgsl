struct ImageInfo {
    width : u32;
    height : u32;
};

[[group(0), binding(0)]] var<uniform> image_info : ImageInfo;
[[group(1), binding(0)]] var input_texture : texture_2d<f32>;
[[group(1), binding(1)]] var output_texture : texture_storage_2d<rgba8unorm, write>;

[[stage(compute), workgroup_size(16, 16)]]
fn main(
  [[builtin(global_invocation_id)]] global_id : vec3<u32>,
) {
    if(global_id.x >= image_info.width || global_id.y >= image_info.height) {
        return;
    }

    let color = textureLoad(input_texture, vec2<i32>(global_id.xy), 0);
    let inverse = vec4<f32>(vec3<f32>(1.0, 1.0, 1.0) - color.rgb, color.a);

    textureStore(output_texture, vec2<i32>(global_id.xy), inverse);
}