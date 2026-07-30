#version 450

// Regenerate the checked-in SPIR-V with:
// glslc -fshader-stage=vertex --target-env=vulkan1.0 -O \
//   ohos_native_buffer.vert -o ohos_native_buffer.vert.spv

layout(location = 0) out vec2 texture_coordinate;

void main() {
    const vec2 positions[3] = vec2[3](
        vec2(-1.0, -1.0),
        vec2( 3.0, -1.0),
        vec2(-1.0,  3.0)
    );
    const vec2 texture_coordinates[3] = vec2[3](
        vec2(0.0, 0.0),
        vec2(2.0, 0.0),
        vec2(0.0, 2.0)
    );
    gl_Position = vec4(positions[gl_VertexIndex], 0.0, 1.0);
    texture_coordinate = texture_coordinates[gl_VertexIndex];
}
