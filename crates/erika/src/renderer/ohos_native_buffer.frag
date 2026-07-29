#version 450

// Regenerate the checked-in SPIR-V with:
// glslc -fshader-stage=fragment --target-env=vulkan1.0 -O \
//   ohos_native_buffer.frag -o ohos_native_buffer.frag.spv

layout(set = 0, binding = 0) uniform sampler2D decoded_frame;

layout(push_constant) uniform CropTransform {
    vec4 offset_scale;
} crop;

layout(location = 0) in vec2 texture_coordinate;
layout(location = 0) out vec4 output_color;

void main() {
    vec2 coordinate = crop.offset_scale.xy + texture_coordinate * crop.offset_scale.zw;
    output_color = texture(decoded_frame, coordinate);
}
