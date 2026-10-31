# Third-Party Notices

QuickVideoMakerは、以下の第三者ソフトウェアを利用しています。各ソフトウェアにはそれぞれのライセンスが適用されます。

## FFmpeg

動画生成エンジンとしてFFmpeg 9.0.2のWindows 64bit静的ビルド（Gyan.dev essentials build）を同梱しています。このビルドはGNU General Public License version 3 or later（GPL-3.0-or-later）の条件で提供されます。取得するアーカイブと実行ファイルの固定SHA-256は`scripts/ffmpeg-release.json`で管理しています。

- Project: https://ffmpeg.org/
- Source: https://github.com/FFmpeg/FFmpeg
- License: https://www.gnu.org/licenses/gpl-3.0.html
- Binary distributor information: https://www.gyan.dev/ffmpeg/builds/
- Binary release: https://github.com/GyanD/codexffmpeg/releases/tag/9.0.2
- Release source: https://github.com/FFmpeg/FFmpeg/tree/n9.0.2

FFmpegはQuickVideoMaker起動時に、EXEと同じ場所の`QuickVideoMaker-PortableData/cache/tools/ffmpeg.exe`へ展開され、独立した実行ファイルとして起動されます。

## C2PA Rust SDK

Content CredentialsのManifest読取・暗号検証に、Content Authenticity InitiativeのC2PA Rust SDKを利用しています。ネットワーク取得機能は無効にし、動画内に埋め込まれたManifestをローカルで処理します。

- Project: https://github.com/contentauth/c2pa-rs
- License: MIT OR Apache-2.0

## Tauri and Rust crates

Tauri、tauri-plugin-single-instance、c2pa、rfd、serde、regex、sha2、same-file、tempfile、windows-sysおよびそれらの依存クレートを利用しています。主にApache-2.0またはMIT Licenseで提供されます。正確な対象と条件は`src-tauri/Cargo.lock`および各クレートの配布物を参照してください。

## Frontend packages

`@tauri-apps/api`を実行時に利用し、ViteとTypeScriptを開発・ビルド時に利用しています。これらはMIT Licenseで提供されます。

## FFmpeg source and build information

https://github.com/aki-sho/QuickVideoMaker/releases/download/v1.7.1/FFmpeg-9.0.2-source-and-build-info.zip

The archive contains the provider-identified FFmpeg source commit 946fcce07b, build configuration, binary hashes, and license text. See third-party/ffmpeg-source.json for checksums and provenance.
