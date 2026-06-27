# ArtCNN weights

[中文](README.zh.md) | [English](README.md) | [日本語](README.ja.md)

`artcnn_c4f16.bin` / `artcnn_c4f32.bin` は [ArtCNN](https://github.com/Artoriuz/ArtCNN) の upstream ONNX release から変換したものです（MIT、`LICENSE.ArtCNN` を参照）。取得元は 2026-06-11 の `main` branch です。

C-series モデルは luma doubler です（1 channel input、2x resolution output）。anime / line-art 向けに Manga109 で学習されています。

| Blob | Architecture | Parameters |
|------|--------------|------------|
| `artcnn_c4f16.bin` | 7 convs, 16 features, residual, DepthToSpace 2x | ~12K |
| `artcnn_c4f32.bin` | 7 convs, 32 features, residual, DepthToSpace 2x | ~48K |

`export_artcnn.py` で再生成できます（`onnx`、`onnxruntime`、`numpy` が必要です）。

```sh
python3 export_artcnn.py ArtCNN_C4F16.onnx artcnn_c4f16.bin \
    --test-vector ../../tests/data/artcnn/c4f16
```

blob layout はスクリプトの header に記載されており、`src/renderer/metal/upscaler.rs` から利用されています。

