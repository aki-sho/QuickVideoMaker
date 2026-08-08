# QuickVideoMaker

QuickVideoMakerは、音楽ファイルと1枚の画像から、音楽の長さに合うMP4動画を作成するWindows 10/11（64bit）向けTauri 2デスクトップアプリです。

作成した動画はアプリ内でプレビューでき、開始・終了時間を指定してカット版を保存できます。既存のMP4、MOV、M4V、AVI、MKV、WebM、WMV動画をインポートして同じカット機能を使うこともできます。カット後の動画は互換性の高いH.264/AACのMP4として書き出します。

## 必要な開発環境

- Windows 10または11（64bit）
- Node.js 20以降とnpm
- Rust stable（MSVC toolchain）
- Microsoft Edge WebView2 Runtime

FFmpegは`ffmpeg-static`から取得され、リリースEXE内へ埋め込まれます。利用者によるFFmpegのインストールは不要です。

## 開発

```powershell
npm install
npm start
```

`npm start`がViteとTauriの開発アプリを起動します。

## 検査

```powershell
npm run check
npm test
```

## ポータブル正式版

```powershell
npm run release:portable
```

このコマンドは内部で`tauri build --no-bundle`を実行し、`dist`へ次の4ファイルだけを生成します。

- `QuickVideoMaker-Portable-1.0.0.exe`
- `QuickVideoMaker-Portable-1.0.0.exe.sha256`
- `QuickVideoMaker-Portable-1.0.0.zip`
- `QuickVideoMaker-Portable-1.0.0.zip.sha256`

ZIPにはバージョン付きフォルダーが1つあり、その中にEXEと`README.txt`が入ります。

## ポータブルデータ

設定、ログ、キャッシュ、一時ファイル、WebViewデータはすべて、実行中のEXEと同じ場所に作成される`QuickVideoMaker-PortableData`内へ保存されます。アプリ固有データをAppData、レジストリ、Program Filesへ保存する処理はありません。

## バージョン管理

アプリの正式なバージョンは`package.json`の`version`です。Tauri設定は同ファイルを参照し、成果物名と画面表示にも同じ値を利用します。

## ライセンス

アプリ本体はMIT Licenseです。同梱するFFmpegを含む第三者ソフトウェアについては[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)を参照してください。
