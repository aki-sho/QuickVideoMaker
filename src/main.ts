import "./styles.css";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { calculateWatermarkBox, calculateWatermarkPoints, initialWatermarkSpacing } from "./watermark-layout";
import { MetadataEditor, type VideoMetadata } from "./metadata-editor";

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
  metadata: VideoMetadata;
};

type ImagePreviewData = {
  bytes: number[];
  mimeType: string;
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
let watermarkImagePath = "";
let watermarkPreviewUrl = "";
let currentAppVersion = "";

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
const videoStage = document.querySelector<HTMLElement>("#videoStage")!;
const watermarkPreviewLayer = document.querySelector<HTMLDivElement>("#watermarkPreviewLayer")!;
const watermarkLiveBadge = document.querySelector<HTMLElement>("#watermarkLiveBadge")!;
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
const watermarkImageInput = document.querySelector<HTMLInputElement>("#watermarkImagePath")!;
const selectWatermarkImageButton = document.querySelector<HTMLButtonElement>("#selectWatermarkImage")!;
const clearWatermarkImageButton = document.querySelector<HTMLButtonElement>("#clearWatermarkImage")!;
const watermarkScale = document.querySelector<HTMLSelectElement>("#watermarkScale")!;
const watermarkPosition = document.querySelector<HTMLSelectElement>("#watermarkPosition")!;
const watermarkX = document.querySelector<HTMLInputElement>("#watermarkX")!;
const watermarkY = document.querySelector<HTMLInputElement>("#watermarkY")!;
const watermarkOpacity = document.querySelector<HTMLInputElement>("#watermarkOpacity")!;
const watermarkSpacing = document.querySelector<HTMLInputElement>("#watermarkSpacing")!;
const watermarkAngle = document.querySelector<HTMLInputElement>("#watermarkAngle")!;
const watermarkCount = document.querySelector<HTMLInputElement>("#watermarkCount")!;
const watermarkXValue = document.querySelector<HTMLOutputElement>("#watermarkXValue")!;
const watermarkYValue = document.querySelector<HTMLOutputElement>("#watermarkYValue")!;
const watermarkOpacityValue = document.querySelector<HTMLOutputElement>("#watermarkOpacityValue")!;
const watermarkSpacingValue = document.querySelector<HTMLOutputElement>("#watermarkSpacingValue")!;
const watermarkAngleValue = document.querySelector<HTMLOutputElement>("#watermarkAngleValue")!;
const watermarkCountValue = document.querySelector<HTMLOutputElement>("#watermarkCountValue")!;
const watermarkPositionControls = document.querySelector<HTMLElement>("#watermarkPositionControls")!;
const watermarkOpacityControls = document.querySelector<HTMLElement>("#watermarkOpacityControls")!;
const watermarkSpacingGuide = document.querySelector<HTMLElement>("#watermarkSpacingGuide")!;
const audioVolume = document.querySelector<HTMLInputElement>("#audioVolume")!;
const audioVolumeValue = document.querySelector<HTMLOutputElement>("#audioVolumeValue")!;
const removeOriginalAudio = document.querySelector<HTMLInputElement>("#removeOriginalAudio")!;
const addedAudioInput = document.querySelector<HTMLInputElement>("#addedAudioPath")!;
const selectAddedAudioButton = document.querySelector<HTMLButtonElement>("#selectAddedAudio")!;
const clearAddedAudioButton = document.querySelector<HTMLButtonElement>("#clearAddedAudio")!;
const loopAddedAudio = document.querySelector<HTMLInputElement>("#loopAddedAudio")!;
const metadataReadonlyList = document.querySelector<HTMLElement>("#metadataReadonlyList")!;
const metadataEditableFields = document.querySelector<HTMLElement>("#metadataEditableFields")!;
const metadataEditor = new MetadataEditor(metadataReadonlyList, metadataEditableFields);

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
  watermarkImageInput.value = watermarkImagePath;
  addedAudioInput.value = addedAudioPath;
  createButton.disabled = isProcessing || !paths.music || !paths.image || !paths.output;
  trimButton.disabled = isProcessing || !currentVideo || !isTrimRangeValid();
  previewTransformButton.disabled = isProcessing || !currentVideo || !isTrimRangeValid();
  clearOverlayImageButton.disabled = isProcessing || !overlayImagePath;
  overlayScale.disabled = isProcessing || !overlayImagePath;
  overlayPosition.disabled = isProcessing || !overlayImagePath;
  overlayBackground.disabled = isProcessing || !overlayImagePath;
  clearWatermarkImageButton.disabled = isProcessing || !watermarkImagePath;
  for (const control of [watermarkScale, watermarkPosition, watermarkX, watermarkY, watermarkOpacity, watermarkSpacing, watermarkAngle, watermarkCount]) {
    control.disabled = isProcessing || !watermarkImagePath;
  }
  audioVolume.disabled = isProcessing || removeOriginalAudio.checked;
  removeOriginalAudio.disabled = isProcessing;
  clearAddedAudioButton.disabled = isProcessing || !addedAudioPath;
  loopAddedAudio.disabled = isProcessing || !addedAudioPath;
  metadataEditor.setDisabled(isProcessing);

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

function selectedOutputDimensions() {
  return selectedAspectRatio() === "9:16" ? { width: 1080, height: 1920 } : { width: 1920, height: 1080 };
}

function watermarkBox() {
  return calculateWatermarkBox(selectedOutputDimensions(), watermarkScale.value as OverlayScale);
}

function setWatermarkCoordinate(input: HTMLInputElement, value: number) {
  const bounded = Math.max(Number(input.min), Math.min(Number(input.max), Math.round(value)));
  input.value = String(bounded);
}

function updateWatermarkBounds() {
  const dimensions = selectedOutputDimensions();
  const box = watermarkBox();
  watermarkX.max = String(Math.max(0, dimensions.width - box.width));
  watermarkY.max = String(Math.max(0, dimensions.height - box.height));
  setWatermarkCoordinate(watermarkX, Number(watermarkX.value));
  setWatermarkCoordinate(watermarkY, Number(watermarkY.value));
}

function applyWatermarkPositionPreset() {
  updateWatermarkBounds();
  const dimensions = selectedOutputDimensions();
  const box = watermarkBox();
  const maxX = Math.max(0, dimensions.width - box.width);
  const maxY = Math.max(0, dimensions.height - box.height);
  const marginX = Math.min(maxX, Math.max(8, Math.floor(dimensions.width / 40)));
  const marginY = Math.min(maxY, Math.max(8, Math.floor(dimensions.height / 40)));
  const positions: Record<OverlayPosition, [number, number]> = {
    "top-left": [marginX, marginY],
    "top-right": [maxX - marginX, marginY],
    center: [maxX / 2, maxY / 2],
    "bottom-left": [marginX, maxY - marginY],
    "bottom-right": [maxX - marginX, maxY - marginY],
  };
  const [x, y] = positions[watermarkPosition.value as OverlayPosition];
  setWatermarkCoordinate(watermarkX, x);
  setWatermarkCoordinate(watermarkY, y);
  updateWatermarkPreview();
}

function watermarkPoints() {
  return calculateWatermarkPoints({
    dimensions: selectedOutputDimensions(),
    box: watermarkBox(),
    x: Number(watermarkX.value),
    y: Number(watermarkY.value),
    spacing: Number(watermarkSpacing.value),
    count: Number(watermarkCount.value),
  });
}

function updateWatermarkOutputs() {
  watermarkXValue.value = `${watermarkX.value}px`;
  watermarkYValue.value = `${watermarkY.value}px`;
  watermarkOpacityValue.value = `${watermarkOpacity.value}%`;
  watermarkSpacingValue.value = `${watermarkSpacing.value}px`;
  watermarkAngleValue.value = `${watermarkAngle.value}°`;
  watermarkCountValue.value = `${watermarkCount.value}個`;
}

function updateVideoStage() {
  videoStage.dataset.aspect = selectedAspectRatio();
  previewVideo.style.objectFit = selectedContentMode();
  updateWatermarkBounds();
}

function updateWatermarkPreview() {
  updateVideoStage();
  updateWatermarkOutputs();
  watermarkPreviewLayer.replaceChildren();
  const enabled = Boolean(watermarkImagePath && watermarkPreviewUrl);
  watermarkLiveBadge.hidden = !enabled;
  if (!enabled) return;

  const dimensions = selectedOutputDimensions();
  const box = watermarkBox();
  for (const point of watermarkPoints()) {
    const item = document.createElement("span");
    item.className = "watermark-preview-item";
    item.style.left = `${point.x / dimensions.width * 100}%`;
    item.style.top = `${point.y / dimensions.height * 100}%`;
    item.style.width = `${box.width / dimensions.width * 100}%`;
    item.style.height = `${box.height / dimensions.height * 100}%`;
    item.style.opacity = String(Number(watermarkOpacity.value) / 100);
    item.style.transform = `rotate(${watermarkAngle.value}deg)`;
    const image = document.createElement("img");
    image.src = watermarkPreviewUrl;
    image.alt = "";
    item.append(image);
    watermarkPreviewLayer.append(item);
  }
}

function releaseWatermarkPreviewUrl() {
  if (watermarkPreviewUrl) URL.revokeObjectURL(watermarkPreviewUrl);
  watermarkPreviewUrl = "";
}

function waitForImagePreview(url: string) {
  return new Promise<void>((resolve, reject) => {
    const image = new Image();
    image.addEventListener("load", () => resolve(), { once: true });
    image.addEventListener("error", () => reject(new Error("選択した画像を画面に表示できません。")), { once: true });
    image.src = url;
  });
}

function resetWatermarkSettings(result: VideoResult) {
  releaseWatermarkPreviewUrl();
  watermarkImagePath = "";
  watermarkScale.value = "small";
  watermarkPosition.value = "bottom-right";
  watermarkOpacity.value = "50";
  watermarkAngle.value = "15";
  watermarkCount.value = "10";
  const spacing = initialWatermarkSpacing(result.width);
  watermarkSpacing.value = String(spacing);
  watermarkSpacingGuide.textContent = `元動画の横幅${result.width}pxから、間隔の初期値を${spacing}pxに設定しました。`;
  applyWatermarkPositionPreset();
}

function selectedWatermark() {
  if (!watermarkImagePath) return null;
  return {
    imagePath: watermarkImagePath,
    scale: watermarkScale.value as OverlayScale,
    position: watermarkPosition.value as OverlayPosition,
    x: Number(watermarkX.value),
    y: Number(watermarkY.value),
    opacity: Number(watermarkOpacity.value),
    spacing: Number(watermarkSpacing.value),
    angle: Number(watermarkAngle.value),
    count: Number(watermarkCount.value),
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
  metadataEditor.populate(result.metadata, result.outputPath, currentAppVersion);
  const recommendedRatio: OutputAspectRatio = result.height > result.width ? "9:16" : "16:9";
  const ratioInput = document.querySelector<HTMLInputElement>(`input[name="outputAspect"][value="${recommendedRatio}"]`);
  if (ratioInput) ratioInput.checked = true;
  resetWatermarkSettings(result);
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
  updateWatermarkPreview();
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
for (const input of document.querySelectorAll<HTMLInputElement>('input[name="outputAspect"]')) {
  input.addEventListener("change", () => {
    markTransformPreviewOutdated();
    applyWatermarkPositionPreset();
  });
}
for (const input of document.querySelectorAll<HTMLInputElement>('input[name="contentMode"]')) {
  input.addEventListener("change", () => {
    markTransformPreviewOutdated();
    updateWatermarkPreview();
  });
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

watermarkScale.addEventListener("change", applyWatermarkPositionPreset);
watermarkPosition.addEventListener("change", applyWatermarkPositionPreset);
for (const input of [watermarkX, watermarkY, watermarkOpacity, watermarkSpacing, watermarkAngle, watermarkCount]) {
  input.addEventListener("input", updateWatermarkPreview);
}

watermarkPositionControls.addEventListener("click", (event) => {
  if (!(event.target instanceof HTMLInputElement)) watermarkPositionControls.focus();
});
watermarkPositionControls.addEventListener("keydown", (event) => {
  if (event.target !== watermarkPositionControls || !watermarkImagePath) return;
  const movement: Record<string, [number, number]> = {
    ArrowLeft: [-1, 0],
    ArrowRight: [1, 0],
    ArrowUp: [0, -1],
    ArrowDown: [0, 1],
  };
  const delta = movement[event.key];
  if (!delta) return;
  event.preventDefault();
  setWatermarkCoordinate(watermarkX, Number(watermarkX.value) + delta[0]);
  setWatermarkCoordinate(watermarkY, Number(watermarkY.value) + delta[1]);
  updateWatermarkPreview();
});

watermarkOpacityControls.addEventListener("click", (event) => {
  if (!(event.target instanceof HTMLInputElement)) watermarkOpacityControls.focus();
});
watermarkOpacityControls.addEventListener("keydown", (event) => {
  if (!watermarkImagePath || (event.key !== "1" && event.key !== "3")) return;
  event.preventDefault();
  const delta = event.key === "1" ? 1 : -1;
  setWatermarkCoordinate(watermarkOpacity, Number(watermarkOpacity.value) + delta);
  updateWatermarkPreview();
});

document.addEventListener("keydown", (event) => {
  if (!watermarkImagePath || event.ctrlKey || event.altKey || event.metaKey) return;
  const active = document.activeElement;
  if (active instanceof HTMLInputElement && ["text", "number"].includes(active.type)) return;
  const shortcuts: Record<string, HTMLInputElement> = {
    "4": watermarkSpacing,
    "5": watermarkAngle,
    "6": watermarkCount,
  };
  const target = shortcuts[event.key];
  if (!target) return;
  event.preventDefault();
  target.focus();
});

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

selectWatermarkImageButton.addEventListener("click", async () => {
  try {
    const selected = await invoke<string | null>("select_image_file");
    if (!selected) return;
    const preview = await invoke<ImagePreviewData>("load_image_preview", { imagePath: selected });
    const nextPreviewUrl = URL.createObjectURL(new Blob([new Uint8Array(preview.bytes)], { type: preview.mimeType }));
    try {
      await waitForImagePreview(nextPreviewUrl);
    } catch (error) {
      URL.revokeObjectURL(nextPreviewUrl);
      throw error;
    }
    releaseWatermarkPreviewUrl();
    watermarkImagePath = selected;
    watermarkPreviewUrl = nextPreviewUrl;
    applyWatermarkPositionPreset();
    syncForm();
    setProgress(0, "ウォーターマークを選択しました。動画へリアルタイム表示しています。");
  } catch (error) {
    setProgress(0, `ウォーターマークを選択できませんでした: ${String(error)}`, "error");
  }
});

clearWatermarkImageButton.addEventListener("click", () => {
  watermarkImagePath = "";
  releaseWatermarkPreviewUrl();
  updateWatermarkPreview();
  syncForm();
  setProgress(0, "ウォーターマークを解除しました。");
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
  updateWatermarkPreview();
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
        watermark: selectedWatermark(),
        metadata: metadataEditor.collect(),
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
    currentAppVersion = version;
    document.querySelector("#appVersion")!.textContent = `QuickVideoMaker v${version}`;
  } catch {
    document.querySelector("#appVersion")!.textContent = "QuickVideoMaker";
  }

  updateWatermarkPreview();
  syncForm();
}

window.addEventListener("beforeunload", releaseWatermarkPreviewUrl);
initialize().catch((error) => setProgress(0, String(error), "error"));
