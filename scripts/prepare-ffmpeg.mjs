import { existsSync, statSync } from "node:fs";
import ffmpegPath from "ffmpeg-static";

if (!ffmpegPath || !existsSync(ffmpegPath)) {
  console.error("ffmpeg-static のWindowsバイナリが見つかりません。npm install を再実行してください。");
  process.exit(1);
}

if (process.platform !== "win32" || !ffmpegPath.toLowerCase().endsWith(".exe")) {
  console.error("このプロジェクトのリリース作成はWindows 64bit専用です。");
  process.exit(1);
}

const sizeMiB = (statSync(ffmpegPath).size / 1024 / 1024).toFixed(1);
console.log(`Bundled FFmpeg: ${ffmpegPath} (${sizeMiB} MiB)`);
