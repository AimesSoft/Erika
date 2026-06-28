# Erika のビルド

Erika は一連の**静的ビルドされたネイティブ依存**（FFmpeg と、オプションで libass 字幕
スタック）をリンクする Rust workspace です。これらのネイティブライブラリは vendoring
されていません——`xtask` オーケストレータで一度ビルドすると `third_party/dist/` 配下に
配置され、Rust crate がそのステージングディレクトリをリンクします。

```
xtask deps build  ──▶  third_party/dist/<target>/<profile>/{ffmpeg,zlib,libass,…}
                                        │
                          erika_ffmpeg_sys/build.rs（dist を自動発見、bindgen 実行）
                                        │
                                  cargo build -p erika
```

> 英語版：[building.md](building.md)。

## 前提

### Rust

- Rust **1.92+**（workspace edition 2024）。
- クロスターゲットでは対応する Rust std target を追加：
  `rustup target add aarch64-apple-ios` や
  `rustup target add x86_64-pc-windows-msvc`。

### ビルドツール —— macOS / Unix ホスト

`tar`、`make`、`clang`、`cmake`、`pkg-config`、`python3`（`venv` 付き）が `PATH` 上に
必要です。完全な字幕スタック（`--all`）には加えて `meson` と `ninja`（Intel ホストでは
FFmpeg の x86 アセンブリに `nasm`）が必要です。macOS では Xcode Command Line Tools と、
上記を Homebrew で導入します。

`erika_ffmpeg_sys` は **bindgen** を実行するため `libclang` が必要です。自動で見つから
ない場合は `LIBCLANG_PATH` を設定します。

### ビルドツール —— Windows（`x86_64-pc-windows-msvc`）

- **Visual Studio Build Tools**（MSVC）+ Windows SDK、および **CMake** コンポーネント。
- **POSIX シェル**（Git for Windows または MSYS2）——FFmpeg の `configure` に必要。
- **GNU make**（MSYS2 `make` または MinGW `mingw32-make`）。
- FFmpeg アセンブリに `nasm`。
- `--all` には **Python**（`venv` 付き）。`xtask` が `pkg-config` シムを自動で用意します。

MSVC 環境が有効なシェル（*"x64 Native Tools Command Prompt"* など）からコマンドを実行し、
`xtask` がツールチェーンを見つけられるようにします。

## `xtask` でネイティブ依存をビルド

`xtask` は workspace メンバーで、`cargo run -p xtask -- …` で呼びます。

```sh
# 何がビルドされるか確認（副作用なし）
cargo run -p xtask -- deps plan
cargo run -p xtask -- deps status

# 最小セット（zlib + FFmpeg）—— LGPL profile
cargo run -p xtask -- deps build --profile lgpl

# libass 字幕スタックを含めすべて
cargo run -p xtask -- deps build --all --profile lgpl
```

サブコマンド：`plan`（計画を表示）、`fetch`（ソースのみ取得）、`status`（存在/ビルド
状況）、`build`（取得 + コンパイル）。

### オプション

| フラグ | 値 | 既定 | 意味 |
|--------|----|------|------|
| `--profile` | `lgpl`、`gpl-full` | `lgpl` | FFmpeg ライセンス profile（下記）。 |
| `--target` | ターゲット表参照 | `host` | クロスコンパイル先。 |
| `--all` | — | off | libass + FreeType + HarfBuzz + FriBidi（字幕描画）も。なしなら zlib + FFmpeg のみ。 |
| `--force` | — | off | 最新マーカーがあっても再ビルド。 |
| `--jobs N` | 整数 | 自動 | ネイティブビルドの並列度。 |

### ターゲット

| `--target` | Triple | 備考 |
|------------|--------|------|
| `host` | 現在のマシン | 既定。 |
| `aarch64-apple-darwin` | Apple Silicon macOS | |
| `x86_64-apple-darwin` | Intel macOS | |
| `aarch64-apple-ios` | iOS 実機 | |
| `aarch64-apple-ios-sim` | iOS sim（Apple Silicon） | |
| `x86_64-apple-ios` | iOS sim（Intel） | |
| `x86_64-pc-windows-msvc`（または `windows-x64`） | Windows | FFmpeg で VideoToolbox を D3D11VA/DXVA2 に置換。 |

デプロイ最小バージョンは既定で macOS `11.0` / iOS `13.0`。
`MACOSX_DEPLOYMENT_TARGET` / `IPHONEOS_DEPLOYMENT_TARGET` で上書き可能。

## ライセンス profile

ネイティブビルドはライセンス境界を明示するため profile で分かれています：

- **`lgpl`**（既定）—— FFmpeg を `--disable-gpl --enable-version3`、静的、ネットワーク
  なし、file プロトコルのみ、厳選した demuxer/decoder/parser セット、zlib 有効、加えて
  VideoToolbox（Apple）または D3D11VA/DXVA2（Windows）で構成。
- **`gpl-full`** —— 同じセットに `--enable-gpl`。成果物の GPL 条項を受け入れる場合のみ。

Rust workspace 自体は MPL-2.0（[`LICENSE`](../LICENSE)）。`xtask` と `cargo build` で
profile を一致させます。`cargo run -p xtask -- check license` がポリシーを検証します。

## `dist` レイアウト

ビルド後、ライブラリは（target + profile ごとに）次に配置されます：

```
third_party/
  cache/                       ダウンロードしたアーカイブ
  src/                         展開したソース
  build/<target>/<profile>/    out-of-tree ビルドツリー
  dist/<target>/<profile>/     crate がリンクする install prefix:
    ffmpeg/{include,lib}
    zlib/    libass/    freetype/    harfbuzz/    fribidi/
```

`host` ターゲットでは `<target>` のパスセグメントは省略されます
（`third_party/dist/<profile>/…`）。

## crate が `dist` を見つける仕組み

`erika_ffmpeg_sys/build.rs` が FFmpeg prefix を自動発見します：

1. `ERIKA_FFMPEG_DIR`（設定されていれば明示上書き）。
2. なければ `third_party/dist/$ERIKA_NATIVE_TARGET/$ERIKA_NATIVE_PROFILE/ffmpeg`
   （`ERIKA_NATIVE_TARGET` が設定されている場合）。
3. なければ workspace ルート配下の `third_party/dist/<profile>/ffmpeg`（iOS 向けビルド
   では `ios/` セグメント付き）。

関連する環境変数：`ERIKA_NATIVE_PROFILE`、`ERIKA_NATIVE_TARGET`、`ERIKA_FFMPEG_DIR`、
`ERIKA_ZLIB_DIR`、`LIBCLANG_PATH`、`ERIKA_ALLOW_LEGACY_FFMPEG`（脱出ハッチ）。Erika は
FFmpeg **7.x**（`libavutil >= 59`）を要求し、Windows ネイティブコアはこれを強制します。
`ERIKA_ALLOW_LEGACY_FFMPEG=1` はローカルの互換性実験のときだけ設定してください。

## コンパイルとテスト

```sh
cargo build -p erika                 # コアライブラリ
cargo build -p erika_capi            # C ABI（dylib/staticlib/dll を生成）
cargo test --workspace               # ユニット + 統合テスト
```

`erika_capi` はネイティブホストがリンクする成果物を生成します：

- macOS：`liberika_capi.dylib`（macOS Flutter プラグインが `dlopen` で読み込む。
  `ERIKA_CAPI_DYLIB` で上書き）。
- iOS：`liberika_capi.a`（静的）。
- Windows：`erika_capi.dll`（Flutter Windows プラグインが `build_erika_runtime.cmake`
  でビルド）。

## 再生パスの検証

```sh
# macOS
export SAMPLE="/path/to/video.mp4"
cargo run -p macos_native_demo -- "$SAMPLE"
cargo run -p macos_native_demo -- --smoke-seconds 3 "$SAMPLE"

# Windows
cargo run -p windows_native_demo -- "%SAMPLE%"
cargo run -p windows_native_demo -- --smoke-seconds 3 --metrics-log out.jsonl "%SAMPLE%"
```

demo はフレームごとのパイプライン統計（デコード/描画フレーム、ゼロコピー vs CPU
フォールバック、HDR10 のアクティブ状況、オーディオ underflow）を出力します——ハード
デコードとゼロコピー相互運用が効いているか手早く確認できます。

## トラブルシューティング

- **「FFmpeg headers were not found …」** —— その target/profile で `xtask deps build` を
  実行していないか、`ERIKA_NATIVE_TARGET`/`ERIKA_NATIVE_PROFILE` がビルドしたものと不一致。
  `deps status` で存在を確認。
- **bindgen / libclang エラー** —— `LIBCLANG_PATH` を LLVM の `lib` ディレクトリに設定。
- **Windows：configure 失敗** —— POSIX シェル（Git Bash/MSYS2）と GNU make が `PATH` 上に
  あり、MSVC 環境から起動していることを確認。
- **旧 FFmpeg が拒否される** —— 7.x バンドルを導入/ビルド。システム FFmpeg に依存しない。
- **license チェック失敗** —— profile に GPL と LGPL の成果物が混在。単一 `--profile` で
  deps を再ビルド。

開発ワークフローは [CONTRIBUTING.ja.md](../CONTRIBUTING.ja.md)、各部分の組み合わせ方は
[architecture.ja.md](architecture.ja.md) を参照してください。
