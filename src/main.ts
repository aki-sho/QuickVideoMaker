import "./styles.css";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";

type Paths = {
  music: string;
  image: string;
  output: string;
};

type ProgressPayload = {
  percent: number;
  message: string;
};

type VideoResult = {
  outputPath: string;
  durationSeconds: number;
  width: number;
  height: number;
};

type OutputAspectRatio = "16:9" | "9:16";

const paths: Paths = { music: "", image: "", output: "" };
let isProcessing = false;
let currentVideo: VideoResult | null = null;

const musicInput = document.querySelector<HTMLInputElement>("#musicPath")!;
const imageInput = document.querySelector<HTMLInputElement>("#imagePath")!;
const outputInput = document.querySelector<HTMLInputElement>("#outputPath")!;
const createButton = document.querySelector<HTMLButtonElement>("#createVideo")!;
const progressBar = document.querySelector<HTMLDivElement>("#progressBar")!;
const progressTrack = document.querySelector<HTMLDivElement>(".progress-track")!;
const progressText = document.querySelector<HTMLSpanElement>("#progressText")!;
const statusMessage = document.querySelector<HTMLParagraphElement>("#statusMessage")!;
const previewSection = document.querySelector<HTMLElement>("#previewSection")!;
const previewVideo = document.querySelector<HTMLVideoElement>("#videoPreview")!;
const previewFileName = document.querySelector<HTMLParagraphElement>("#previewFileName")!;
const videoDuration = document.querySelector<HTMLSpanElement>("#videoDuration")!;
const videoDimensions = document.querySelector<HTMLElement>("#videoDimensions")!;
const trimStart = document.querySelector<HTMLInputElement>("#trimStart")!;
const trimEnd = document.querySelector<HTMLInputElement>("#trimEnd")!;
const trimmedDuration = document.querySelector<HTMLElement>("#trimmedDuration")!;
const trimButton = document.querySelector<HTMLButtonElement>("#trimAndSave")!;

function setProgress(percent: number, message: string, kind: "normal" | "success" | "error" = "normal") {
  const bounded = Math.max(0, Math.min(100, Math.round(percent)));
  progressBar.style.width = `${bounded}%`;
  progressText.textContent = `${bounded}%`;
  progressTrack.setAttribute("aria-valuenow", String(bounded));
  statusMessage.textContent = message;
  statusMessage.dataset.kind = kind;
}

function syncForm() {
  musicInput.value = paths.music;
  imageInput.value = paths.image;
  outputInput.value = paths.output;
  createButton.disabled = isProcessing || !paths.music || !paths.image || !paths.output;
  trimButton.disabled = isProcessing || !currentVideo || !isTrimRangeValid();

  for (const button of document.querySelectorAll<HTMLButtonElement>("button.secondary, button.text-button")) {
    button.disabled = isProcessing;
  }
  trimStart.disabled = isProcessing;
  trimEnd.disabled = isProcessing;
  for (const input of document.querySelectorAll<HTMLInputElement>('input[name="outputAspect"]')) {
    input.disabled = isProcessing;
  }
}

function formatTime(seconds: number) {
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remaining = total % 60;
  return hours > 0
    ? `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}:${String(remaining).padStart(2, "0")}`
    : `${String(minutes).padStart(2, "0")}:${String(remaining).padStart(2, "0")}`;
}

function isTrimRangeValid() {
  if (!currentVideo) return false;
  const start = Number(trimStart.value);
  const end = Number(trimEnd.value);
  return Number.isFinite(start) && Number.isFinite(end) && start >= 0 && end > start && end <= currentVideo.durationSeconds + 0.05;
}

function updateTrimSummary() {
  const start = Number(trimStart.value);
  const end = Number(trimEnd.value);
  const duration = Number.isFinite(start) && Number.isFinite(end) ? Math.max(0, end - start) : 0;
  trimmedDuration.textContent = `${duration.toFixed(1)}秒`;
  syncForm();
}

function suggestedCutName(path: string) {
  const fileName = path.split(/[\\/]/).pop() || "video.mp4";
  return fileName.replace(/\.mp4$/i, "") + "-cut.mp4";
}

function greatestCommonDivisor(first: number, second: number) {
  let left = Math.max(1, Math.round(first));
  let right = Math.max(1, Math.round(second));
  while (right !== 0) {
    [left, right] = [right, left % right];
  }
  return left;
}

function analyzedRatio(result: VideoResult) {
  const divisor = greatestCommonDivisor(result.width, result.height);
  return `${result.width / divisor}:${result.height / divisor}`;
}

function selectedAspectRatio(): OutputAspectRatio {
  const value = document.querySelector<HTMLInputElement>('input[name="outputAspect"]:checked')?.value;
  return value === "9:16" ? "9:16" : "16:9";
}

function showPreview(result: VideoResult) {
  currentVideo = result;
  previewSection.hidden = false;
  previewFileName.textContent = result.outputPath;
  videoDuration.textContent = formatTime(result.durationSeconds);
  videoDimensions.textContent = `${result.width} × ${result.height}（${analyzedRatio(result)}）`;
  const recommendedRatio: OutputAspectRatio = result.height > result.width ? "9:16" : "16:9";
  const ratioInput = document.querySelector<HTMLInputElement>(`input[name="outputAspect"][value="${recommendedRatio}"]`);
  if (ratioInput) ratioInput.checked = true;
  trimStart.value = "0";
  trimStart.max = result.durationSeconds.toFixed(3);
  trimEnd.value = result.durationSeconds.toFixed(1);
  trimEnd.max = result.durationSeconds.toFixed(3);
  previewVideo.src = `${convertFileSrc("preview", "qvm")}?v=${Date.now()}`;
  previewVideo.load();
  updateTrimSummary();
  previewSection.scrollIntoView({ behavior: "smooth", block: "start" });
}

async function choose(kind: "music" | "image" | "output") {
  try {
    const command = {
      music: "select_music_file",
      image: "select_image_file",
      output: "select_output_file",
    }[kind];
    const selected = await invoke<string | null>(command, {
      suggestedName: kind === "output" ? "video.mp4" : undefined,
    });
    if (selected) {
      paths[kind] = selected;
      syncForm();
      setProgress(0, "動画を作成する準備ができました。");
    }
  } catch (error) {
    setProgress(0, String(error), "error");
  }
}

document.querySelector("#selectMusic")!.addEventListener("click", () => choose("music"));
document.querySelector("#selectImage")!.addEventListener("click", () => choose("image"));
document.querySelector("#selectOutput")!.addEventListener("click", () => choose("output"));

document.querySelector("#selectVideo")!.addEventListener("click", async () => {
  try {
    const videoPath = await invoke<string | null>("select_video_file");
    if (!videoPath) return;

    isProcessing = true;
    syncForm();
    setProgress(5, "動画を読み込んでいます…");
    const result = await invoke<VideoResult>("import_video", { videoPath });
    showPreview(result);
    setProgress(100, `動画を読み込みました: ${result.outputPath}`, "success");
  } catch (error) {
    setProgress(0, `動画を読み込めませんでした: ${String(error)}`, "error");
  } finally {
    isProcessing = false;
    syncForm();
  }
});

createButton.addEventListener("click", async () => {
  isProcessing = true;
  syncForm();
  setProgress(1, "動画作成を開始しています…");

  try {
    const result = await invoke<VideoResult>("create_video", {
      request: {
        audioPath: paths.music,
        imagePath: paths.image,
        outputPath: paths.output,
      },
    });
    showPreview(result);
    setProgress(100, `動画を保存しました: ${paths.output}`, "success");
  } catch (error) {
    setProgress(0, `作成できませんでした: ${String(error)}`, "error");
  } finally {
    isProcessing = false;
    syncForm();
  }
});

trimStart.addEventListener("input", updateTrimSummary);
trimEnd.addEventListener("input", updateTrimSummary);

document.querySelector("#setTrimStart")!.addEventListener("click", () => {
  trimStart.value = Math.min(previewVideo.currentTime, Number(trimEnd.value) - 0.1).toFixed(1);
  updateTrimSummary();
});

document.querySelector("#setTrimEnd")!.addEventListener("click", () => {
  const duration = currentVideo?.durationSeconds ?? previewVideo.duration;
  trimEnd.value = Math.max(previewVideo.currentTime, Number(trimStart.value) + 0.1).toFixed(1);
  if (Number(trimEnd.value) > duration) trimEnd.value = duration.toFixed(1);
  updateTrimSummary();
});

previewVideo.addEventListener("loadedmetadata", () => {
  if (!currentVideo || !Number.isFinite(previewVideo.duration)) return;
  currentVideo.durationSeconds = previewVideo.duration;
  videoDuration.textContent = formatTime(previewVideo.duration);
  trimStart.max = previewVideo.duration.toFixed(3);
  trimEnd.max = previewVideo.duration.toFixed(3);
  if (Number(trimEnd.value) > previewVideo.duration) trimEnd.value = previewVideo.duration.toFixed(1);
  updateTrimSummary();
});

previewVideo.addEventListener("error", () => {
  setProgress(100, "動画は作成されましたが、プレビューを読み込めませんでした。", "error");
});

trimButton.addEventListener("click", async () => {
  if (!currentVideo || !isTrimRangeValid()) {
    setProgress(0, "開始時間と終了時間を確認してください。", "error");
    return;
  }

  try {
    const outputPath = await invoke<string | null>("select_output_file", {
      suggestedName: suggestedCutName(currentVideo.outputPath),
    });
    if (!outputPath) return;

    isProcessing = true;
    syncForm();
    setProgress(1, "動画の長さと比率を変換しています…");
    previewVideo.pause();

    const result = await invoke<VideoResult>("trim_video", {
      request: {
        outputPath,
        startSeconds: Number(trimStart.value),
        endSeconds: Number(trimEnd.value),
        aspectRatio: selectedAspectRatio(),
      },
    });
    showPreview(result);
    setProgress(100, `${selectedAspectRatio()}の動画を保存しました: ${result.outputPath}`, "success");
  } catch (error) {
    setProgress(0, `カットできませんでした: ${String(error)}`, "error");
  } finally {
    isProcessing = false;
    syncForm();
  }
});

async function initialize() {
  await listen<ProgressPayload>("video-progress", ({ payload }) => {
    setProgress(payload.percent, payload.message);
  });

  try {
    const version = await getVersion();
    document.querySelector("#appVersion")!.textContent = `QuickVideoMaker v${version}`;
  } catch {
    document.querySelector("#appVersion")!.textContent = "QuickVideoMaker";
  }

  syncForm();
}

initialize().catch((error) => setProgress(0, String(error), "error"));
