struct Settings {
    filter_size : u32,
};

struct Orientation {
    vertical : u32,
};

@group(0) @binding(0) var<uniform> settings : Settings;
@group(1) @binding(0) var input_texture : texture_2d<f32>;
@group(1) @binding(1) var output_texture : texture_storage_2d<rgba8unorm, write>;
@group(1) @binding(2) var<uniform> orientation: Orientation;

@compute
@workgroup_size(128)
fn main(
  @builtin(global_invocation_id) global_id : vec3<u32>,
) {
    let filter_radius = i32((settings.filter_size - 1u) / 2u);
    let filter_size = i32(settings.filter_size);
    let dimensions = textureDimensions(input_texture);
    var position = vec2<i32>(global_id.xy);
    if (orientation.vertical == 0u) {
        position = position.yx;
    }
    
    if(position.x >= dimensions.x || position.y >= dimensions.y) {
        return;
    }

    let original = textureLoad(input_texture, position, 0);
    var color : vec4<f32> = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    
    if (orientation.vertical > 0u) {
        for (var i : i32 = position.y - filter_radius; i <= position.y + filter_radius; i = i + 1){
            color = color + (1.0 / f32(filter_size)) * textureLoad(input_texture, vec2<i32>(position.x, i), 0);
        }
    } else {        
        for (var i : i32 = position.x - filter_radius; i <= position.x + filter_radius; i = i + 1){
            color = color + (1.0 / f32(filter_size)) * textureLoad(input_texture, vec2<i32>(i, position.y), 0);
        }
    }

    textureStore(output_texture, position, color);
}