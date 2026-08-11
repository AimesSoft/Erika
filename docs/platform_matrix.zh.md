# Erika 平台能力矩阵

本文把“能编译”“CI 构建覆盖”“真机验收”和“有可下载预编译包”分开描述。任何一项为真，都不自动代表其它三项为真。Erika 当前已支持 NipaPlay 的所有原生客户端目标，**仅 Linux 尚未支持**；在 NipaPlay 的电视设备（包括 tvOS）上，播放器工厂会强制选择 Erika。

| 平台 | 主要解码/渲染 | C ABI presenter | CI/产物 | 真机验收状态 |
|---|---|---|---|---|
| macOS | VideoToolbox + Metal | 是 | CI 与预编译包 | 持续维护；HDR/EDR 依显示器和系统而定 |
| iOS | VideoToolbox + Metal | 是 | CI 与 XCFramework | 持续维护；需使用真机验证 HDR/后台行为 |
| tvOS | VideoToolbox + Metal | 是 | CI 与 XCFramework | 支持；NipaPlay 电视端强制使用 Erika，真机与模拟器分别记录 |
| Windows x64/ARM64 | D3D11VA + D3D11 | 是 | CI 与预编译包 | 持续维护；HDR10 受显示器、驱动和系统设置影响 |
| Android | MediaCodec/软解 + wgpu | 是 | CI 与预编译包 | SDR 已验证；API 35 HDR active path 仍需真机验收 |
| HarmonyOS | AVCodec/软解 + wgpu Vulkan | 是 | 预编译包；CI 覆盖需以 workflow 为准 | 已有真机验证，尚未纳入完整 CI 验收 |
| Linux | 规划中的 wgpu 路径 | 不作为发布承诺 | 无正式预编译发布 | 未验收 |

## surface 与嵌入选择

- Apple：优先 native Metal surface；Flutter 完整播放器优先 window overlay，platform view 用于兼容或诊断。
- Windows：使用 HWND/D3D11 attach，调用方负责窗口生命周期与 display tick。
- Android：SDR 使用 TextureView；extended-linear 输出使用 SurfaceView/Hybrid Composition，能力协商失败明确回退 SDR。
- HarmonyOS：ArkTS 外部纹理提供 `OHNativeWindow`，通过 `erika_presenter_attach_wgpu_surface` attach；平台桥接优先使用 JSON presenter helper。

## 发布前记录

发布说明应写明每个平台的目标 triple、FFmpeg profile、prebuilt tag、C header 版本、CI 结果及真机验收设备。没有真机结论时应标记“待验收”，不要写成“已完全支持”。
