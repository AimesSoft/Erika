# Dolby Vision Implementation - Improvements Applied

This document summarizes the improvements made to the Dolby Vision RPU mapping implementation based on the code review.

## Changes Made

### 1. Enhanced Documentation

#### Added Comprehensive Top-Level Documentation
- **File**: `docs/dolby-vision.md`
- **Content**:
  - Complete pipeline architecture diagram
  - Profile 5 vs Profile 8 comparison table
  - Reshaping algorithm explanation (polynomial and MMR)
  - Color transform flow description
  - Testing strategy and environment variables
  - Known limitations and performance characteristics
  - Cross-references to implementation files

#### Improved Inline Documentation
- **File**: `crates/erika/src/renderer/pipeline.rs`
- Enhanced `DoviSourceMetadata` struct documentation with field-level descriptions
- Clarified signal offset scaling for the actual uploaded representation (`2^bits/(2^bits-1)`), including the P010-to-NV12 fallback
- Added uniform buffer size note (~3KB, 20% of typical mobile device limits)

### 2. Better Error Handling

#### Validation Comments
- **File**: `crates/erika/src/ffmpeg.rs:4406`
- Added comment explaining validation ranges for Dolby Vision parameters
- Clarified that bit depth must be 8-16 and coefficient denominator <= 32 (FFmpeg standardizes float coefficients using `coef_log2_denom = 32`)
- Original approach with logging would have required adding a new dependency

### 3. CI Documentation

#### Linker Flag Explanation
- **File**: `.github/workflows/ci.yml:130`
- Added inline comments explaining the `libclang_rt.osx` workaround
- Referenced the relevant commits (acae10f, 3498b8e) for historical context
- Clarifies this resolves `__isPlatformVersionAtLeast` symbol issues

### 4. Test Coverage Improvements

#### Profile 8 Testing
- **File**: `crates/erika/src/ffmpeg.rs:5281`
- Added `dv_profile_8_sample_reports_profile` test
- Validates Profile 8 detection via `ERIKA_DV_PROFILE_8_SAMPLE` environment variable
- Complements existing Profile 5 test coverage

#### Decode Fallback Logic Testing
- **File**: `crates/erika/src/playback.rs:8013`
- Added unit tests for `dolby_vision_decode_fallback()`:
  - `dolby_vision_profile_5_stays_on_hardware_for_videotoolbox_and_d3d11va`: Confirms Profile 5 stays on hardware decode for desktop backends where FFmpeg provides RPU side data alongside GPU textures
  - `dolby_vision_profile_5_falls_back_to_software_on_mobile_backends`: Verifies Profile 5 falls back to software decode on mobile backends (MediaCodec, AvCodec) where hardware decoders omit RPU metadata
  - `dolby_vision_profile_8_stays_on_hardware_decode`: Confirms Profile 8 remains on hardware decode (backward-compatible base layer displays correctly)
  - `dolby_vision_software_decode_stays_software`: Validates no-op behavior when already on software decode

## What Was NOT Changed

### Deferred Improvements

1. **Dynamic Uniform Buffer Allocation**
   - Current ~3KB static allocation works fine for modern GPUs
   - Would add complexity without measurable benefit
   - Can be revisited if mobile device testing reveals issues

2. **Binary Pivot Search**
   - Linear search through 7 pivots is already fast on GPUs
   - Branch divergence might make binary search slower
   - Current implementation matches libplacebo's proven approach

3. **Logging Infrastructure**
   - Project doesn't currently use a logging framework
   - Added comments instead of introducing new dependencies
   - Error cases are rare (malformed streams) and return `None` cleanly

## Testing Instructions

### Running New Tests

```bash
# Profile 8 detection test (requires sample)
export ERIKA_DV_PROFILE_8_SAMPLE=/path/to/dv-profile8-sample.mp4
cargo test dv_profile_8_sample_reports_profile

# Decode fallback unit tests (no samples needed)
cargo test dolby_vision_profile_5_stays_on_hardware_for_videotoolbox_and_d3d11va
cargo test dolby_vision_profile_5_falls_back_to_software_on_mobile_backends
cargo test dolby_vision_profile_8_stays_on_hardware_decode
cargo test dolby_vision_software_decode_stays_software
```

### Existing Tests Still Pass

```bash
# Profile 5 detection
export ERIKA_DV_SAMPLE=/path/to/dv-profile5-sample.mp4
cargo test dv_sample_reports_dolby_vision_profile

# RPU metadata parsing
cargo test frame_reads_dovi_side_data

# Uniform packing
cargo test dovi_uniforms_pack_pivots_poly_and_mmr

# PQ forcing
cargo test dovi_source_forces_pq_when_stream_tags_are_missing

# Shader formula presence
cargo test dovi_formulas_are_present_across_video_shaders
```

## Architecture Validation

The implementation continues to follow best practices:

### ✅ Correctness
- Matches libplacebo's proven implementation
- Proper normalization of pivots and coefficients
- Correct Profile 5 decode fallback policy (keeps hardware decode on desktop where metadata is retained; falls back on mobile)
- Forced BT.2020/PQ prevents VUI tag issues

### ✅ Cross-Platform Consistency
- WGSL, Metal, HLSL shaders implement identical logic
- Uniform buffer layout is vec4-aligned across all backends
- Test coverage ensures formula presence in all shaders

### ✅ Performance
- Zero-copy FFmpeg metadata extraction via pointer arithmetic
- GPU-friendly uniform layout (~3KB total)
- Low CPU overhead: metadata parsing reads fixed-size structures via pointer offsets without full frame copies

### ✅ Maintainability
- Clear separation: FFmpeg → Pipeline → Shader layers
- Comprehensive documentation at top level and inline
- Test coverage for both profiles and edge cases
- Comments reference commits and external specifications

## Known Limitations (Documented)

1. **Dual-Layer FEL / MEL Not Composed**: Ultra HD Blu-ray Profile 7 dual-layer enhancement layers are not composed; non-trivial NLQ or EL residual gracefully falls back to the base layer.

2. **Mobile Hardware Decode RPU Extraction**: On mobile platforms (e.g. Android MediaCodec), hardware decoders drop RPU metadata, requiring software decode for Profile 5.

3. **Uniform Buffer Size**: ~3KB may exceed very old mobile GPU limits (pre-2015 devices).

4. **No Per-Scene Optimization**: Static allocation for worst case (8 segments × 3 orders) to avoid pipeline recompilation.

## References

All improvements maintain alignment with:
- [libplacebo's dovi_reshape](https://github.com/haasn/libplacebo/blob/master/src/shaders/dovi.c)
- [Dolby Vision Specification](https://professional.dolby.com/dolby-vision/)
- [FFmpeg AVDOVIMetadata](https://ffmpeg.org/doxygen/trunk/structAVDOVIMetadata.html)
- mpv's Dolby Vision behavior

## Summary

These improvements enhance **documentation**, **testing**, and **decode correctness** across desktop and mobile platforms with verified test coverage and accurate profile characterization.
