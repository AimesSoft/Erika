# ArtCNN weights

`artcnn_c4f16.bin`, `artcnn_c4f16_ds.bin`, and `artcnn_c4f32.bin` are converted from the upstream ONNX
releases of [ArtCNN](https://github.com/Artoriuz/ArtCNN) (MIT, see
`LICENSE.ArtCNN`). The neutral models were fetched from `main` on 2026-06-11;
the DS model was fetched on 2026-08-11.

The C-series models are luma doublers (1 channel in, 2x resolution out)
trained on Manga109 for anime/line-art content:

| Blob | Architecture | Parameters |
|------|--------------|------------|
| `artcnn_c4f16.bin` | 7 convs, 16 features, residual, DepthToSpace 2x | ~12K |
| `artcnn_c4f16_ds.bin` | C4F16 denoising/sharpening variant for degraded sources | ~12K |
| `artcnn_c4f32.bin` | 7 convs, 32 features, residual, DepthToSpace 2x | ~48K |

Regenerate with `export_artcnn.py` (needs `onnx`, `onnxruntime`, `numpy`):

```sh
python3 export_artcnn.py ArtCNN_C4F16.onnx artcnn_c4f16.bin \
    --test-vector ../../tests/data/artcnn/c4f16
```

The blob layout is documented in the script header and consumed by
`src/renderer/metal/upscaler.rs`.
