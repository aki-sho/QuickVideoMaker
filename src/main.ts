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
type CreateOutputSize = "landscape" | "portrait" | "image";
type ContentMode = "contain" | "cover";
type OverlayScale = "small" | "medium" | "large" | "full";
type OverlayPosition = "top-left" | "top-right" | "center" | "bottom-left" | "bottom-right";
type OverlayBackground = "original" | "white" | "black";

const paths: Paths = { music: "", image: "", output: "" };
let isProcessing = false;
let currentVideo: VideoResult | null = null;
let isShowingTransformPreview = false;
let displayedPreviewOffset = 0;
let overlayImagePath = "";
let addedAudioPath = "";

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
const previewTransformButton = document.querySelector<HTMLButtonElement>("#previewTransform")!;
const overlayImageInput = document.querySelector<HTMLInputElement>("#overlayImagePath")!;
const selectOverlayImageButton = document.querySelector<HTMLButtonElement>("#selectOverlayImage")!;
const clearOverlayImageButton = document.querySelector<HTMLButtonElement>("#clearOverlayImage")!;
const overlayScale = document.querySelector<HTMLSelectElement>("#overlayScale")!;
const overlayPosition = document.querySelector<HTMLSelectElement>("#overlayPosition")!;
const overlayBackground = document.querySelector<HTMLSelectElement>("#overlayBackground")!;
const audioVolume = document.querySelector<HTMLInputElement>("#audioVolume")!;
const audioVolumeValue = document.querySelector<HTMLOutputElement>("#audioVolumeValue")!;
const removeOriginalAudio = document.querySelector<HTMLInputElement>("#removeOriginalAudio")!;
const addedAudioInput = document.querySelector<HTMLInputElement>("#addedAudioPath")!;
const selectAddedAudioButton = document.querySelector<HTMLButtonElement>("#selectAddedAudio")!;
const clearAddedAudioButton = document.querySelector<HTMLButtonElement>("#clearAddedAudio")!;
const loopAddedAudio = document.querySelector<HTMLInputElement>("#loopAddedAudio")!;

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
  overlayImageInput.value = overlayImagePath;
  addedAudioInput.value = addedAudioPath;
  createButton.disabled = isProcessing || !paths.music || !paths.image || !paths.output;
  trimButton.disabled = isProcessing || !currentVideo || !isTrimRangeValid();
  previewTransformButton.disabled = isProcessing || !currentVideo || !isTrimRangeValid();
  clearOverlayImageButton.disabled = isProcessing || !overlayImagePath;
  overlayScale.disabled = isProcessing || !overlayImagePath;
  overlayPosition.disabled = isProcessing || !overlayImagePath;
  overlayBackground.disabled = isProcessing || !overlayImagePath;
  audioVolume.disabled = isProcessing || removeOriginalAudio.checked;
  removeOriginalAudio.disabled = isProcessing;
  clearAddedAudioButton.disabled = isProcessing || !addedAudioPath;
  loopAddedAudio.disabled = isProcessing || !addedAudioPath;

  for (const button of document.querySelectorAll<HTMLButtonElement>("button.secondary, button.text-button")) {
    button.disabled = isProcessing;
  }
  trimStart.disabled = isProcessing;
  trimEnd.disabled = isProcessing;
  for (const input of document.querySelectorAll<HTMLInputElement>('input[name="createOutputSize"], input[name="outputAspect"], input[name="contentMode"]')) {
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
  markTransformPreviewOutdated();
  syncForm();
}

function markTransformPreviewOutdated() {
  if (isShowingTransformPreview) {
    previewFileName.textContent = "変換プレビュー（設定が変更されました）";
  }
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

function selectedCreateOutputSize(): CreateOutputSize {
  const value = document.querySelector<HTMLInputElement>('input[name="createOutputSize"]:checked')?.value;
  if (value === "portrait" || value === "image") return value;
  return "landscape";
}

function selectedContentMode(): ContentMode {
  const value = document.querySelector<HTMLInputElement>('input[name="contentMode"]:checked')?.value;
  return value === "cover" ? "cover" : "contain";
}

function selectedOverlay() {
  if (!overlayImagePath) return null;
  return {
    imagePath: overlayImagePath,
    scale: overlayScale.value as OverlayScale,
    position: overlayPosition.value as OverlayPosition,
    background: overlayBackground.value as OverlayBackground,
  };
}

function selectedAddedAudio() {
  if (!addedAudioPath) return null;
  return {
    audioPath: addedAudioPath,
    loopAudio: loopAddedAudio.checked,
  };
}

function showPreview(result: VideoResult) {
  currentVideo = result;
  overlayImagePath = "";
  addedAudioPath = "";
  removeOriginalAudio.checked = false;
  loopAddedAudio.checked = true;
  audioVolume.value = "100";
  audioVolumeValue.value = "100";
  isShowingTransformPreview = false;
  displayedPreviewOffset = 0;
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

function showTransformPreview(result: VideoResult) {
  isShowingTransformPreview = true;
  displayedPreviewOffset = Number(trimStart.value);
  previewFileName.textContent = "変換プレビュー（未保存・最大10秒）";
  videoDuration.textContent = formatTime(result.durationSeconds);
  previewVideo.src = `${convertFileSrc("preview", "qvm")}?v=${Date.now()}`;
  previewVideo.load();
  previewVideo.addEventListener("canplay", () => void previewVideo.play(), { once: true });
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
        outputSize: selectedCreateOutputSize(),
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
for (const input of document.querySelectorAll<HTMLInputElement>('input[name="outputAspect"], input[name="contentMode"]')) {
  input.addEventListener("change", markTransformPreviewOutdated);
}
overlayScale.addEventListener("change", markTransformPreviewOutdated);
overlayPosition.addEventListener("change", markTransformPreviewOutdated);
overlayBackground.addEventListener("change", markTransformPreviewOutdated);
audioVolume.addEventListener("input", () => {
  audioVolumeValue.value = audioVolume.value;
  markTransformPreviewOutdated();
});
removeOriginalAudio.addEventListener("change", () => {
  markTransformPreviewOutdated();
  syncForm();
});
loopAddedAudio.addEventListener("change", markTransformPreviewOutdated);

selectAddedAudioButton.addEventListener("click", async () => {
  try {
    const selected = await invoke<string | null>("select_music_file");
    if (!selected) return;
    addedAudioPath = selected;
    markTransformPreviewOutdated();
    syncForm();
    setProgress(0, "追加する音声を選択しました。動画の先頭から再生されます。");
  } catch (error) {
    setProgress(0, `音声を選択できませんでした: ${String(error)}`, "error");
  }
});

clearAddedAudioButton.addEventListener("click", () => {
  addedAudioPath = "";
  markTransformPreviewOutdated();
  syncForm();
  setProgress(0, "追加する音声を解除しました。");
});

selectOverlayImageButton.addEventListener("click", async () => {
  try {
    const selected = await invoke<string | null>("select_image_file");
    if (!selected) return;
    overlayImagePath = selected;
    markTransformPreviewOutdated();
    syncForm();
    setProgress(0, "重ねる画像を選択しました。［設定をプレビュー］で確認できます。");
  } catch (error) {
    setProgress(0, `画像を選択できませんでした: ${String(error)}`, "error");
  }
});

clearOverlayImageButton.addEventListener("click", () => {
  overlayImagePath = "";
  markTransformPreviewOutdated();
  syncForm();
  setProgress(0, "重ねる画像を解除しました。");
});

document.querySelector("#setTrimStart")!.addEventListener("click", () => {
  const sourceTime = displayedPreviewOffset + previewVideo.currentTime;
  trimStart.value = Math.min(sourceTime, Number(trimEnd.value) - 0.1).toFixed(1);
  updateTrimSummary();
});

document.querySelector("#setTrimEnd")!.addEventListener("click", () => {
  const duration = currentVideo?.durationSeconds ?? previewVideo.duration;
  const sourceTime = displayedPreviewOffset + previewVideo.currentTime;
  trimEnd.value = Math.max(sourceTime, Number(trimStart.value) + 0.1).toFixed(1);
  if (Number(trimEnd.value) > duration) trimEnd.value = duration.toFixed(1);
  updateTrimSummary();
});

previewVideo.addEventListener("loadedmetadata", () => {
  syncForm();
});

previewVideo.addEventListener("error", () => {
  setProgress(100, "動画は作成されましたが、プレビューを読み込めませんでした。", "error");
});

previewTransformButton.addEventListener("click", async () => {
  if (!currentVideo || !isTrimRangeValid()) {
    setProgress(0, "開始時間と終了時間を確認してください。", "error");
    return;
  }

  isProcessing = true;
  syncForm();
  setProgress(1, "保存前プレビューを作成しています…");
  previewVideo.pause();

  try {
    const result = await invoke<VideoResult>("render_video_preview", {
      request: {
        startSeconds: Number(trimStart.value),
        endSeconds: Number(trimEnd.value),
        aspectRatio: selectedAspectRatio(),
        contentMode: selectedContentMode(),
        overlay: selectedOverlay(),
        audioVolume: Number(audioVolume.value),
        removeOriginalAudio: removeOriginalAudio.checked,
        addedAudio: selectedAddedAudio(),
      },
    });
    showTransformPreview(result);
    setProgress(100, "保存前プレビューを作成しました。内容を確認してからダウンロードできます。", "success");
  } catch (error) {
    setProgress(0, `プレビューを作成できませんでした: ${String(error)}`, "error");
  } finally {
    isProcessing = false;
    syncForm();
  }
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
        contentMode: selectedContentMode(),
        overlay: selectedOverlay(),
        audioVolume: Number(audioVolume.value),
        removeOriginalAudio: removeOriginalAudio.checked,
        addedAudio: selectedAddedAudio(),
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
