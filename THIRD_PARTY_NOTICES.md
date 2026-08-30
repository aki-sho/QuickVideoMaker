# Third-Party Notices

QuickVideoMakerは、以下の第三者ソフトウェアを利用しています。各ソフトウェアにはそれぞれのライセンスが適用されます。

## FFmpeg

動画生成エンジンとしてFFmpegのWindows 64bit静的ビルドを`ffmpeg-static` npmパッケージ経由で同梱しています。このビルドはGNU General Public License version 3 or later（GPL-3.0-or-later）の条件で提供されます。

- Project: https://ffmpeg.org/
- Source: https://github.com/FFmpeg/FFmpeg
- License: https://www.gnu.org/licenses/gpl-3.0.html
- Binary distributor information: https://github.com/eugeneware/ffmpeg-static

FFmpegはQuickVideoMaker起動時に、EXEと同じ場所の`QuickVideoMaker-PortableData/cache/tools/ffmpeg.exe`へ展開され、独立した実行ファイルとして起動されます。

## C2PA Rust SDK

Content CredentialsのManifest読取・暗号検証に、Content Authenticity InitiativeのC2PA Rust SDKを利用しています。ネットワーク取得機能は無効にし、動画内に埋め込まれたManifestをローカルで処理します。

- Project: https://github.com/contentauth/c2pa-rs
- License: MIT OR Apache-2.0

## Tauri and Rust crates

Tauri、tauri-plugin-single-instance、c2pa、rfd、serde、regex、sha2およびそれらの依存クレートを利用しています。主にApache-2.0またはMIT Licenseで提供されます。正確な対象と条件は`src-tauri/Cargo.lock`および各クレートの配布物を参照してください。

## Frontend packages

`@tauri-apps/api`を実行時に利用し、ViteとTypeScriptを開発・ビルド時に利用しています。これらはMIT Licenseで提供されます。`ffmpeg-static`のパッケージコードはMIT Licenseです（同梱FFmpegバイナリには上記FFmpegのライセンスが適用されます）。
