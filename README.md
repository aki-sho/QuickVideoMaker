# QuickVideoMaker

QuickVideoMakerは、音楽ファイルと1枚の画像から、音楽の長さに合うMP4動画を作成するWindows 10/11（64bit）向けTauri 2デスクトップアプリです。作成サイズは1920×1080、1080×1920、選択画像と同じサイズから選べます。

作成した動画はアプリ内でプレビューでき、開始・終了時間を指定してカット版を保存できます。既存のMP4、MOV、M4V、AVI、MKV、WebM、WMV動画をインポートして同じ編集機能を使うこともできます。編集画面は「出力比率」「中身の画像表示」「画像を重ねる」「カット」「元の音声」の5項目に分かれています。元音声は100を元の音量として0～100で調整でき、音声トラック自体の削除にも対応します。新しい音声を動画先頭から追加し、元音声とのミックスまたは置き換え、動画終了までのループを選択できます。任意画像のサイズ・位置・白黒下地にも対応し、スクロール中も固定表示される最大10秒プレビューで仕上がりを確認できます。「画像を重ねる」とは別に、必ず最前面へ表示するウォーターマークを追加できます。画像を選択した時点で動画上へ表示され、プレビューボタンを押さずにサイズ・位置・X/Y座標・透過率・間隔・角度・個数をリアルタイム調整できます。開閉式のメタデータ管理では、フレームレート・コーデック・色空間などを確認し、Title・Creator・日時・独自タグ・XMPなどを編集または削除できます。さらに「生成元を示す証明情報」を開くと、公式C2PA SDKが動画内のContent Credentialsをローカル検証し、検証結果、生成ソフト・機器、署名者、履歴、AI申告、Manifest ID、仕様版、元素材、警告・エラーなど11項目を確認できます。出力は互換性の高いH.264/AACのMP4です。

## 必要な開発環境

- Windows 10または11（64bit）
- Node.js 20以降とnpm
- Rust 1.88以降のstable（MSVC toolchain）
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

- `QuickVideoMaker-Portable-1.7.0.exe`
- `QuickVideoMaker-Portable-1.7.0.exe.sha256`
- `QuickVideoMaker-Portable-1.7.0.zip`
- `QuickVideoMaker-Portable-1.7.0.zip.sha256`

ZIPにはバージョン付きフォルダーが1つあり、その中にEXEと`README.txt`が入ります。

## ポータブルデータ

設定、ログ、キャッシュ、一時ファイル、WebViewデータはすべて、実行中のEXEと同じ場所に作成される`QuickVideoMaker-PortableData`内へ保存されます。アプリ固有データをAppData、レジストリ、Program Filesへ保存する処理はありません。

## バージョン管理

アプリの正式なバージョンは`package.json`の`version`です。Tauri設定は同ファイルを参照し、成果物名と画面表示にも同じ値を利用します。

## ライセンス

アプリ本体はMIT Licenseです。同梱するFFmpegを含む第三者ソフトウェアについては[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)を参照してください。
