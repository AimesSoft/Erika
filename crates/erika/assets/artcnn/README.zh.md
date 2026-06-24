# ArtCNN 权重

[中文](README.zh.md) | [English](README.md) | [日本語](README.ja.md)

`artcnn_c4f16.bin` / `artcnn_c4f32.bin` 是从 [ArtCNN](https://github.com/Artoriuz/ArtCNN) 的上游 ONNX release 转换而来（MIT，见 `LICENSE.ArtCNN`），来源是 2026-06-11 抓取的 `main` 分支。

C 系列模型是亮度 doubler（1 通道输入，2x 分辨率输出），针对动漫/线稿内容在 Manga109 上训练：

| Blob | Architecture | Parameters |
|------|--------------|------------|
| `artcnn_c4f16.bin` | 7 convs, 16 features, residual, DepthToSpace 2x | ~12K |
| `artcnn_c4f32.bin` | 7 convs, 32 features, residual, DepthToSpace 2x | ~48K |

使用 `export_artcnn.py` 可重新生成（需要 `onnx`、`onnxruntime`、`numpy`）：

```sh
python3 export_artcnn.py ArtCNN_C4F16.onnx artcnn_c4f16.bin \
    --test-vector ../../tests/data/artcnn/c4f16
```

blob 布局记录在脚本头部，并由 `src/renderer/metal/upscaler.rs` 消费。

