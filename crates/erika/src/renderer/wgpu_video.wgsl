// WGSL port of the Metal `VIDEO_SHADER_SOURCE` in `renderer/metal/apple.rs`.
// Kept line-for-line equivalent so the wgpu backend produces the same pixels as
// the native Metal renderer for a given frame and color pipeline.

struct VideoUniforms {
    is_p010: u32,
    full_range: u32,
    source_transfer: u32,
    target_transfer: u32,
    tone_map: u32,
    edr_output: u32,
    input_mode: u32,
    reserved1: u32,
    nits: vec4<f32>,
    luma_coefficients: vec4<f32>,
    gamut_matrix_rows: array<vec4<f32>, 3>,
};

@group(0) @binding(0) var<uniform> uniforms: VideoUniforms;
@group(0) @binding(1) var luma_texture: texture_2d<f32>;
@group(0) @binding(2) var chroma_texture: texture_2d<f32>;
@group(0) @binding(3) var video_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
};

fn source_peak_nits() -> f32 {
    return max(uniforms.nits.x, 1.0);
}

fn target_peak_nits() -> f32 {
    return max(uniforms.nits.y, 1.0);
}

fn source_reference_white_nits() -> f32 {
    return max(uniforms.nits.z, 1.0);
}

fn target_reference_white_nits() -> f32 {
    return max(uniforms.nits.w, 1.0);
}

fn pq_eotf(encoded: f32) -> f32 {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;
    let p = pow(max(encoded, 0.0), 1.0 / m2);
    let num = max(p - c1, 0.0);
    let den = max(c2 - c3 * p, 0.000001);
    return pow(num / den, 1.0 / m1);
}

fn pq_inverse_eotf(normalized_nits: f32) -> f32 {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;
    let p = pow(clamp(normalized_nits, 0.0, 1.0), m1);
    return pow((c1 + c2 * p) / max(1.0 + c3 * p, 0.000001), m2);
}

// BT.2100 HLG inverse OETF: nonlinear signal E' to scene linear light in
// [0, 1]. Mirrors the Rust reference implementation in
// `renderer/pipeline.rs` tests (`hlg_inverse_oetf`).
fn hlg_inverse_oetf(encoded: f32) -> f32 {
    let a = 0.17883277;
    let b = 0.28466892;
    let c = 0.55991073;
    let e = max(encoded, 0.0);
    if (e <= 0.5) {
        return e * e / 3.0;
    }
    return (exp((e - c) / a) + b) / 12.0;
}

fn transfer_to_source_reference_linear(rgb_in: vec3<f32>) -> vec3<f32> {
    let rgb = max(rgb_in, vec3<f32>(0.0));
    if (uniforms.source_transfer == 3u) {
        let pq_absolute_peak_nits = 10000.0;
        return vec3<f32>(pq_eotf(rgb.r), pq_eotf(rgb.g), pq_eotf(rgb.b))
            * (pq_absolute_peak_nits / source_reference_white_nits());
    }
    if (uniforms.source_transfer == 4u) {
        // HLG: inverse OETF to scene linear, then the BT.2100 OOTF (system
        // gamma 1.2 at the 1000 nit nominal peak) to display linear,
        // normalized to source reference white like the PQ branch above.
        let hlg_nominal_peak_nits = 1000.0;
        let hlg_system_gamma = 1.2;
        let scene = vec3<f32>(
            hlg_inverse_oetf(rgb.r),
            hlg_inverse_oetf(rgb.g),
            hlg_inverse_oetf(rgb.b)
        );
        let scene_luma = max(dot(uniforms.luma_coefficients.xyz, scene), 0.000001);
        return scene * (hlg_nominal_peak_nits * pow(scene_luma, hlg_system_gamma - 1.0)
            / source_reference_white_nits());
    }
    if (uniforms.source_transfer == 1u) {
        return pow(rgb, vec3<f32>(2.2));
    }
    if (uniforms.source_transfer == 2u) {
        return pow(rgb, vec3<f32>(2.4));
    }
    return rgb;
}

fn source_reference_to_nits(rgb: vec3<f32>) -> vec3<f32> {
    return max(rgb, vec3<f32>(0.0)) * source_reference_white_nits();
}

fn tone_map_nits(nits: vec3<f32>) -> vec3<f32> {
    let source_peak = source_peak_nits();
    let target_peak = target_peak_nits();
    let x = max(nits, vec3<f32>(0.0)) / target_peak;
    let white = max(source_peak / target_peak, 1.0);
    if (uniforms.tone_map == 1u) {
        let white2 = white * white;
        return target_peak * clamp((x * (vec3<f32>(1.0) + x / white2)) / (vec3<f32>(1.0) + x), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (uniforms.tone_map == 2u) {
        let knee = 0.75;
        let denom = max(white - knee, 0.0001);
        let t = clamp((x - vec3<f32>(knee)) / denom, vec3<f32>(0.0), vec3<f32>(1.0));
        let shoulder = knee + (1.0 - knee) * (vec3<f32>(1.0) - pow(vec3<f32>(1.0) - t, vec3<f32>(2.0)));
        return target_peak * mix(x, shoulder, step(vec3<f32>(knee), x));
    }
    return target_peak * clamp(x, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn apply_gamut_map(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(uniforms.gamut_matrix_rows[0].xyz, rgb),
        dot(uniforms.gamut_matrix_rows[1].xyz, rgb),
        dot(uniforms.gamut_matrix_rows[2].xyz, rgb)
    );
}

fn target_nits_to_reference_linear(nits: vec3<f32>) -> vec3<f32> {
    return max(nits, vec3<f32>(0.0)) / target_reference_white_nits();
}

fn target_reference_linear_to_output(rgb: vec3<f32>) -> vec3<f32> {
    if (uniforms.target_transfer == 3u) {
        let pq_absolute_peak_nits = 10000.0;
        let nits = max(rgb, vec3<f32>(0.0)) * target_reference_white_nits();
        return vec3<f32>(
            pq_inverse_eotf(nits.r / pq_absolute_peak_nits),
            pq_inverse_eotf(nits.g / pq_absolute_peak_nits),
            pq_inverse_eotf(nits.b / pq_absolute_peak_nits)
        );
    }
    if (uniforms.edr_output != 0u) {
        return max(rgb, vec3<f32>(0.0));
    }
    if (uniforms.target_transfer == 1u) {
        return pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    }
    if (uniforms.target_transfer == 2u) {
        return pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4));
    }
    return rgb;
}

fn final_output(rgb: vec3<f32>) -> vec4<f32> {
    if (uniforms.target_transfer == 3u) {
        return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
    }
    if (uniforms.edr_output != 0u) {
        let headroom = max(target_peak_nits() / target_reference_white_nits(), 1.0);
        return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(headroom)), 1.0);
    }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

struct RangeExpandedYCbCr {
    y: f32,
    cbcr: vec2<f32>,
};

fn expand_ycbcr_range(y_in: f32, cbcr_in: vec2<f32>) -> RangeExpandedYCbCr {
    var out: RangeExpandedYCbCr;
    if (uniforms.full_range != 0u) {
        out.y = y_in;
        out.cbcr = cbcr_in - vec2<f32>(0.5);
        return out;
    }
    if (uniforms.is_p010 != 0u) {
        out.y = (y_in - (64.0 / 1023.0)) * (1023.0 / 876.0);
        out.cbcr = (cbcr_in - vec2<f32>(512.0 / 1023.0)) * (1023.0 / 896.0);
        return out;
    }
    out.y = (y_in - (16.0 / 255.0)) * (255.0 / 219.0);
    out.cbcr = (cbcr_in - vec2<f32>(128.0 / 255.0)) * (255.0 / 224.0);
    return out;
}

fn packed_luma_texel(virtual_coord_in: vec2<i32>, virtual_size: vec2<i32>) -> f32 {
    let virtual_coord = clamp(virtual_coord_in, vec2<i32>(0), virtual_size - vec2<i32>(1));
    let packed_coord = virtual_coord / vec2<i32>(2);
    let packed = textureLoad(luma_texture, packed_coord, 0);
    let component = u32((virtual_coord.y & 1) * 2 + (virtual_coord.x & 1));
    if (component == 0u) {
        return packed.r;
    }
    if (component == 1u) {
        return packed.g;
    }
    if (component == 2u) {
        return packed.b;
    }
    return packed.a;
}

// ArtCNN stores the four DCR subpixels of a virtual 2W x 2H luma image in one
// RGBA texel. Reconstruct the same normalized-coordinate bilinear sample that
// a native 2W x 2H texture would provide, without allocating that large image.
fn sample_packed_luma(tex_coord: vec2<f32>) -> f32 {
    let packed_size = vec2<i32>(textureDimensions(luma_texture, 0));
    let virtual_size = packed_size * vec2<i32>(2);
    let sample_position = clamp(tex_coord, vec2<f32>(0.0), vec2<f32>(1.0))
        * vec2<f32>(virtual_size) - vec2<f32>(0.5);
    let lo = vec2<i32>(floor(sample_position));
    let fraction = fract(sample_position);
    let y0 = mix(
        packed_luma_texel(lo, virtual_size),
        packed_luma_texel(lo + vec2<i32>(1, 0), virtual_size),
        fraction.x,
    );
    let y1 = mix(
        packed_luma_texel(lo + vec2<i32>(0, 1), virtual_size),
        packed_luma_texel(lo + vec2<i32>(1, 1), virtual_size),
        fraction.x,
    );
    return mix(y0, y1, fraction.y);
}

@vertex
fn erika_video_vertex(@builtin(vertex_index) vertex_id: u32) -> VertexOut {
    // Avoid dynamically indexing a function-local position array here. The
    // Android emulator's SwiftShader GLES 3.0 compiler accepts Naga's generated
    // GLSL for that pattern but rasterizes no vertices. These bits produce the
    // same three full-screen-triangle coordinates without an array lookup.
    let unit = vec2<f32>(
        f32(vertex_id & 1u),
        f32((vertex_id >> 1u) & 1u),
    );
    var out: VertexOut;
    out.position = vec4<f32>(unit * 4.0 - vec2<f32>(1.0), 0.0, 1.0);
    out.tex_coord = vec2<f32>(unit.x * 2.0, 1.0 - unit.y * 2.0);
    return out;
}

@fragment
fn erika_video_fragment(in: VertexOut) -> @location(0) vec4<f32> {
    var rgb: vec3<f32>;
    if (uniforms.input_mode == 1u) {
        rgb = textureSample(luma_texture, video_sampler, in.tex_coord).rgb;
    } else if (uniforms.input_mode == 3u) {
        let original_rgb = textureSample(chroma_texture, video_sampler, in.tex_coord).rgb;
        let original_luma = dot(uniforms.luma_coefficients.xyz, original_rgb);
        let enhanced_luma = sample_packed_luma(in.tex_coord);
        rgb = original_rgb + vec3<f32>(enhanced_luma - original_luma);
    } else {
        var y_sample: f32;
        if (uniforms.input_mode == 2u) {
            y_sample = sample_packed_luma(in.tex_coord);
        } else {
            y_sample = textureSample(luma_texture, video_sampler, in.tex_coord).r;
        }
        let cbcr_sample = textureSample(chroma_texture, video_sampler, in.tex_coord).rg;
        let expanded = expand_ycbcr_range(y_sample, cbcr_sample);
        let y = expanded.y;
        let cbcr = expanded.cbcr;

        let kr = uniforms.luma_coefficients.x;
        let kg = max(uniforms.luma_coefficients.y, 0.000001);
        let kb = uniforms.luma_coefficients.z;
        rgb.r = y + 2.0 * (1.0 - kr) * cbcr.y;
        rgb.b = y + 2.0 * (1.0 - kb) * cbcr.x;
        rgb.g = (y - kr * rgb.r - kb * rgb.b) / kg;
    }
    rgb = transfer_to_source_reference_linear(rgb);
    rgb = apply_gamut_map(rgb);
    rgb = source_reference_to_nits(rgb);
    rgb = tone_map_nits(rgb);
    rgb = target_nits_to_reference_linear(rgb);
    rgb = target_reference_linear_to_output(rgb);
    return final_output(rgb);
}
