import { createHash, randomUUID } from "node:crypto";
import { createReadStream, createWriteStream, existsSync, mkdirSync, readFileSync, renameSync, rmSync, statSync } from "node:fs";
import { pipeline } from "node:stream/promises";
import { Readable, Transform } from "node:stream";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const release = JSON.parse(readFileSync(join(root, "scripts", "ffmpeg-release.json"), "utf8"));
const tools = join(root, ".tools", "ffmpeg");
const binary = join(tools, "ffmpeg.exe");

async function sha256(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function verifiedBinary() {
  if (!existsSync(binary) || await sha256(binary) !== release.binarySha256) return false;
  const check = spawnSync(binary, ["-version"], { encoding: "utf8", windowsHide: true, timeout: 10_000, maxBuffer: 64 * 1024 });
  return check.status === 0 && check.stdout.startsWith(`ffmpeg version ${release.version}-`);
}

if (process.platform !== "win32" || process.arch !== "x64") throw new Error("FFmpegの準備はWindows 64bit専用です。");
if (!/^[a-f0-9]{64}$/.test(release.binarySha256)) throw new Error("FFmpegの固定ハッシュが不正です。");
mkdirSync(tools, { recursive: true });
if (!await verifiedBinary()) {
  const archive = join(tools, `download-${randomUUID()}.zip`);
  const extracted = join(tools, `ffmpeg-${randomUUID()}.exe`);
  try {
    console.log(`Downloading verified FFmpeg ${release.version}...`);
    const response = await fetch(release.url, { signal: AbortSignal.timeout(180_000) });
    if (!response.ok || !response.body) throw new Error(`FFmpegの取得に失敗しました: HTTP ${response.status}`);
    let size = 0;
    const limiter = new Transform({ transform(chunk, _, done) {
      size += chunk.length;
      done(size > 256 * 1024 * 1024 ? new Error("FFmpegアーカイブが上限を超えました。") : null, chunk);
    } });
    await pipeline(Readable.fromWeb(response.body), limiter, createWriteStream(archive, { flags: "wx" }));
    if (await sha256(archive) !== release.archiveSha256) throw new Error("FFmpegアーカイブのSHA-256が一致しません。");
    const extraction = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-File", join(root, "scripts", "extract-ffmpeg.ps1"), "-ArchivePath", archive, "-EntryName", release.entry, "-Destination", extracted], { windowsHide: true, encoding: "utf8", timeout: 60_000 });
    if (extraction.status !== 0) throw new Error(`FFmpegを展開できません: ${extraction.stderr || extraction.error || "unknown error"}`);
    if (await sha256(extracted) !== release.binarySha256) throw new Error("FFmpeg本体のSHA-256が一致しません。");
    rmSync(binary, { force: true });
    renameSync(extracted, binary);
  } finally {
    rmSync(archive, { force: true });
    rmSync(extracted, { force: true });
  }
  if (!await verifiedBinary()) throw new Error("FFmpegのバージョン検証に失敗しました。");
}
console.log(`Bundled FFmpeg ${release.version}: SHA-256 verified (${(statSync(binary).size / 1024 / 1024).toFixed(1)} MiB)`);
