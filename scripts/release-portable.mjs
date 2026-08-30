import { createHash } from "node:crypto";
import { createReadStream, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, copyFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(readFileSync(join(projectRoot, "package.json"), "utf8"));
const productName = "QuickVideoMaker";
const artifactBase = `${productName}-Portable-${packageJson.version}`;
const distDir = join(projectRoot, "dist");
const stagingRoot = join(projectRoot, ".release-staging");
const stagingFolder = join(stagingRoot, artifactBase);
const builtExe = join(projectRoot, "src-tauri", "target", "release", "quick-video-maker.exe");
const portableExe = join(distDir, `${artifactBase}.exe`);
const zipPath = join(distDir, `${artifactBase}.zip`);

function removeGeneratedDirectory(path) {
  rmSync(path, { recursive: true, force: true, maxRetries: 12, retryDelay: 250 });
}

function emptyGeneratedDirectory(path) {
  mkdirSync(path, { recursive: true });
  for (const entry of readdirSync(path)) {
    removeGeneratedDirectory(join(path, entry));
  }
}

function run(program, args) {
  const result = spawnSync(program, args, { cwd: projectRoot, stdio: "inherit", shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function quotePowerShell(value) {
  return `'${value.replaceAll("'", "''")}'`;
}

async function sha256(path) {
  const hash = createHash("sha256");
  await new Promise((resolvePromise, reject) => {
    createReadStream(path).on("data", (chunk) => hash.update(chunk)).on("end", resolvePromise).on("error", reject);
  });
  return hash.digest("hex");
}

if (process.platform !== "win32" || process.arch !== "x64") {
  console.error("release:portable はWindows 64bit環境で実行してください。");
  process.exit(1);
}

run(process.execPath, [join(projectRoot, "scripts", "prepare-ffmpeg.mjs")]);
run(process.execPath, [join(projectRoot, "node_modules", "@tauri-apps", "cli", "tauri.js"), "build", "--no-bundle"]);

if (!existsSync(builtExe)) {
  throw new Error(`Tauriのビルド結果が見つかりません: ${builtExe}`);
}

emptyGeneratedDirectory(distDir);
removeGeneratedDirectory(stagingRoot);
mkdirSync(distDir, { recursive: true });
mkdirSync(stagingFolder, { recursive: true });

copyFileSync(builtExe, portableExe);
copyFileSync(builtExe, join(stagingFolder, basename(portableExe)));

const portableReadme = `QuickVideoMaker ${packageJson.version}\r\n\r\n` +
  "使い方\r\n" +
  "1. EXEを任意の書き込み可能なフォルダーへ展開します。\r\n" +
  "2. 新規作成する場合は、音楽、画像、作成サイズ、保存先を選び［動画作成］を押します。\r\n" +
  "3. 既存動画をカットする場合は［動画をインポート］から動画を選びます。\r\n" +
  "4. 元動画の解析結果を確認し、16:9または9:16の出力比率を選びます。\r\n" +
  "5. 中身は［全体を表示］または［枠いっぱい］から選びます。\r\n" +
  "6. 必要なら［画像を重ねる］で画像・サイズ・位置を指定します。\r\n" +
  "7. 元動画を隠す場合は［白で隠す］または［黒で隠す］を選びます。\r\n" +
  "8. 必要なら［ウォーターマーク］で画像、位置、透過、間隔、角度、個数を調整します。\r\n" +
  "9. 開始・終了時間と元の音声（0～100）を指定します。\r\n" +
  "10. 必要なら元音声を削除し、新しい音声とループ設定を選びます。\r\n" +
  "11. ［設定をプレビュー］で仕上がりを確認します。\r\n" +
  "12. ［ダウンロード］を押して保存先を選ぶと、H.264/AACのMP4として保存されます。\r\n\r\n" +
  "アプリのデータ\r\n" +
  "設定、ログ、キャッシュ、一時ファイル、WebViewデータは、EXEと同じ場所の\r\n" +
  "QuickVideoMaker-PortableData フォルダーに保存されます。アンインストールは\r\n" +
  "EXEとこのフォルダーを削除してください。\r\n\r\n" +
  "動作環境: Windows 10/11 64bit、Microsoft Edge WebView2 Runtime\r\n\r\n" +
  "本製品はFFmpegを同梱しています。FFmpegはGNU GPL v3以降の条件で提供されます。\r\n" +
  "FFmpeg: https://ffmpeg.org/\r\n" +
  "ソースコード: https://github.com/FFmpeg/FFmpeg\r\n";
writeFileSync(join(stagingFolder, "README.txt"), portableReadme, "utf8");

const command = `$ErrorActionPreference='Stop'; Compress-Archive -LiteralPath ${quotePowerShell(stagingFolder)} -DestinationPath ${quotePowerShell(zipPath)} -CompressionLevel Optimal`;
run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command]);

const exeHash = await sha256(portableExe);
const zipHash = await sha256(zipPath);
writeFileSync(`${portableExe}.sha256`, `${exeHash}  ${basename(portableExe)}\n`, "ascii");
writeFileSync(`${zipPath}.sha256`, `${zipHash}  ${basename(zipPath)}\n`, "ascii");
removeGeneratedDirectory(stagingRoot);

console.log("Portable release created:");
for (const file of [portableExe, `${portableExe}.sha256`, zipPath, `${zipPath}.sha256`]) {
  console.log(`  ${file}`);
}
