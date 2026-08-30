use crate::metadata::{self, EditableMetadata, VideoMetadata};
use crate::{portable::PortablePaths, state::ProcessControl};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVideoRequest {
    audio_path: String,
    image_path: String,
    output_path: String,
    output_size: CreateOutputSize,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreateOutputSize {
    Landscape,
    Portrait,
    Image,
}

impl CreateOutputSize {
    fn video_filter(self) -> &'static str {
        match self {
            Self::Landscape => "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,setsar=1,format=yuv420p",
            Self::Portrait => "scale=1080:1920:force_original_aspect_ratio=decrease,pad=1080:1920:(ow-iw)/2:(oh-ih)/2,setsar=1,format=yuv420p",
            Self::Image => "scale=trunc(iw/2)*2:trunc(ih/2)*2,setsar=1,format=yuv420p",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Landscape => "1920x1080",
            Self::Portrait => "1080x1920",
            Self::Image => "image-size",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimVideoRequest {
    output_path: String,
    start_seconds: f64,
    end_seconds: f64,
    aspect_ratio: OutputAspectRatio,
    content_mode: ContentMode,
    overlay: Option<OverlaySettings>,
    watermark: Option<WatermarkSettings>,
    metadata: EditableMetadata,
    audio_volume: u8,
    remove_original_audio: bool,
    added_audio: Option<AddedAudioSettings>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewVideoRequest {
    start_seconds: f64,
    end_seconds: f64,
    aspect_ratio: OutputAspectRatio,
    content_mode: ContentMode,
    overlay: Option<OverlaySettings>,
    audio_volume: u8,
    remove_original_audio: bool,
    added_audio: Option<AddedAudioSettings>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedAudioSettings {
    audio_path: String,
    loop_audio: bool,
}

struct ValidatedAddedAudio {
    audio_path: PathBuf,
    loop_audio: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySettings {
    image_path: String,
    scale: OverlayScale,
    position: OverlayPosition,
    background: OverlayBackground,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayBackground {
    Original,
    White,
    Black,
}

impl OverlayBackground {
    fn color(self) -> Option<&'static str> {
        match self {
            Self::Original => None,
            Self::White => Some("white"),
            Self::Black => Some("black"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::White => "white",
            Self::Black => "black",
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayScale {
    Small,
    Medium,
    Large,
    Full,
}

impl OverlayScale {
    fn percent(self) -> u32 {
        match self {
            Self::Small => 20,
            Self::Medium => 35,
            Self::Large => 50,
            Self::Full => 100,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Full => "full",
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPosition {
    TopLeft,
    TopRight,
    Center,
    BottomLeft,
    BottomRight,
}

impl OverlayPosition {
    fn coordinates(self, margin_x: u32, margin_y: u32) -> String {
        match self {
            Self::TopLeft => format!("{margin_x}:{margin_y}"),
            Self::TopRight => format!("W-w-{margin_x}:{margin_y}"),
            Self::Center => "(W-w)/2:(H-h)/2".to_string(),
            Self::BottomLeft => format!("{margin_x}:H-h-{margin_y}"),
            Self::BottomRight => format!("W-w-{margin_x}:H-h-{margin_y}"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::Center => "center",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
        }
    }
}

struct ValidatedOverlay {
    image_path: PathBuf,
    scale: OverlayScale,
    position: OverlayPosition,
    background: OverlayBackground,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkSettings {
    image_path: String,
    scale: OverlayScale,
    position: OverlayPosition,
    x: u32,
    y: u32,
    opacity: u8,
    spacing: u32,
    angle: i16,
    count: u8,
}

struct ValidatedWatermark {
    image_path: PathBuf,
    scale: OverlayScale,
    position: OverlayPosition,
    x: u32,
    y: u32,
    opacity: u8,
    spacing: u32,
    angle: i16,
    count: u8,
}

#[derive(Clone, Copy, Deserialize)]
pub enum OutputAspectRatio {
    #[serde(rename = "16:9")]
    Landscape,
    #[serde(rename = "9:16")]
    Portrait,
}

impl OutputAspectRatio {
    fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Landscape => (1920, 1080),
            Self::Portrait => (1080, 1920),
        }
    }

    fn preview_dimensions(self) -> (u32, u32) {
        match self {
            Self::Landscape => (640, 360),
            Self::Portrait => (360, 640),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Landscape => "16:9",
            Self::Portrait => "9:16",
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentMode {
    Contain,
    Cover,
}

impl ContentMode {
    fn video_filter(self, width: u32, height: u32) -> String {
        match self {
            Self::Contain => format!(
                "scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,format=yuv420p"
            ),
            Self::Cover => format!(
                "scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height},setsar=1,format=yuv420p"
            ),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Contain => "contain",
            Self::Cover => "cover",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoResult {
    pub output_path: String,
    pub duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub metadata: VideoMetadata,
}

#[derive(Clone, Serialize)]
struct ProgressPayload {
    percent: u8,
    message: String,
}

struct BusyGuard(Arc<ProcessControl>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

pub fn generate(
    app: AppHandle,
    paths: PortablePaths,
    process: Arc<ProcessControl>,
    request: CreateVideoRequest,
) -> Result<VideoResult, String> {
    if process
        .busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("別の動画を作成中です。".to_string());
    }
    let _busy_guard = BusyGuard(process.clone());

    if process.shutting_down.load(Ordering::SeqCst) {
        return Err("アプリを終了しています。".to_string());
    }

    let audio = validate_input(&request.audio_path, "音楽")?;
    let image = validate_input(&request.image_path, "画像")?;
    let output = validate_output(&request.output_path, &audio, &image)?;
    let ffmpeg = paths.ffmpeg_path()?;
    let session = paths.session_temp()?;
    let temporary_output = session.join("video-output.mp4");

    emit_progress(&app, 2, "同梱FFmpegを準備しました。");
    paths.log(&format!(
        "video creation started: {} ({})",
        output.display(),
        request.output_size.label()
    ));

    let mut command = Command::new(&ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-y")
        .arg("-loop")
        .arg("1")
        .arg("-framerate")
        .arg("30")
        .arg("-i")
        .arg(&image)
        .arg("-i")
        .arg(&audio)
        .arg("-vf")
        .arg(request.output_size.video_filter())
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("medium")
        .arg("-tune")
        .arg("stillimage")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("192k")
        .arg("-shortest")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(&temporary_output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .env("TEMP", &paths.temp)
        .env("TMP", &paths.temp);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("FFmpegを起動できません: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "FFmpegの進捗を取得できません".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "FFmpegの出力を取得できません".to_string())?;

    {
        let mut active = process
            .child
            .lock()
            .map_err(|_| "プロセス管理を開始できません".to_string())?;
        *active = Some(child);
    }

    let duration_ms = Arc::new(AtomicU64::new(0));
    let error_lines = Arc::new(Mutex::new(VecDeque::<String>::with_capacity(24)));
    let duration_for_stderr = duration_ms.clone();
    let errors_for_stderr = error_lines.clone();
    let stderr_thread = thread::spawn(move || {
        let duration_pattern = Regex::new(r"Duration: (\d{2}):(\d{2}):(\d{2}(?:\.\d+)?)")
            .expect("valid duration regex");
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if duration_for_stderr.load(Ordering::Relaxed) == 0 {
                if let Some(captures) = duration_pattern.captures(&line) {
                    let hours = captures[1].parse::<f64>().unwrap_or_default();
                    let minutes = captures[2].parse::<f64>().unwrap_or_default();
                    let seconds = captures[3].parse::<f64>().unwrap_or_default();
                    duration_for_stderr.store(
                        ((hours * 3600.0 + minutes * 60.0 + seconds) * 1000.0) as u64,
                        Ordering::Relaxed,
                    );
                }
            }
            if let Ok(mut lines) = errors_for_stderr.lock() {
                if lines.len() == 24 {
                    lines.pop_front();
                }
                lines.push_back(line);
            }
        }
    });

    let app_for_stdout = app.clone();
    let duration_for_stdout = duration_ms.clone();
    let stdout_thread = thread::spawn(move || {
        let mut last_percent = 0_u8;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(value) = line.strip_prefix("out_time_us=") {
                let elapsed_ms = value.parse::<u64>().unwrap_or_default() / 1000;
                let duration = duration_for_stdout.load(Ordering::Relaxed);
                let percent = if duration > 0 {
                    (5.0 + (elapsed_ms as f64 / duration as f64 * 90.0)).min(95.0) as u8
                } else {
                    5
                };
                if percent > last_percent {
                    last_percent = percent;
                    emit_progress(
                        &app_for_stdout,
                        percent,
                        "映像と音声をエンコードしています…",
                    );
                }
            }
        }
    });

    let exit_status = loop {
        let result = {
            let mut active = process
                .child
                .lock()
                .map_err(|_| "プロセス状態を確認できません".to_string())?;
            match active.as_mut() {
                Some(child) => child.try_wait(),
                None => return Err("動画作成プロセスが中断されました。".to_string()),
            }
        };
        match result {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(format!("FFmpegの状態を確認できません: {error}")),
        }
    };

    if let Ok(mut active) = process.child.lock() {
        active.take();
    }
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    if !exit_status.success() {
        let details = error_lines
            .lock()
            .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        let _ = fs::remove_dir_all(&session);
        paths.log(&format!("ffmpeg failed: {details}"));
        return Err(format!(
            "FFmpegがエラーを返しました。\n{}",
            compact_error(&details)
        ));
    }

    emit_progress(&app, 97, "完成した動画を保存しています…");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("保存先フォルダーを作成できません: {error}"))?;
    }
    fs::copy(&temporary_output, &output)
        .map_err(|error| format!("動画を保存できません: {error}"))?;
    let _ = fs::remove_dir_all(&session);
    paths.log(&format!("video creation completed: {}", output.display()));
    emit_progress(&app, 100, "動画を作成しました。");
    inspect_video(&paths, &output.to_string_lossy())
}

pub fn inspect_video(paths: &PortablePaths, value: &str) -> Result<VideoResult, String> {
    let input = validate_input(value, "動画")?;
    let ffmpeg = paths.ffmpeg_path()?;
    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-i")
        .arg(&input)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    hide_console_window(&mut command);

    let output = command
        .output()
        .map_err(|error| format!("動画情報を取得できません: {error}"))?;
    let details = String::from_utf8_lossy(&output.stderr);
    if !details.contains("Video:") {
        return Err("映像ストリームを含む動画ファイルを選択してください。".to_string());
    }
    let duration_seconds = parse_duration_seconds(&details)
        .filter(|duration| *duration > 0.0)
        .ok_or_else(|| "動画の長さを取得できません。".to_string())?;
    let (width, height) = parse_video_dimensions(&details)
        .ok_or_else(|| "動画の幅と高さを取得できません。".to_string())?;

    let metadata = metadata::inspect(paths, &input, &details, width, height);
    Ok(VideoResult {
        output_path: input.to_string_lossy().into_owned(),
        duration_seconds,
        width,
        height,
        metadata,
    })
}

pub fn trim(
    app: AppHandle,
    paths: PortablePaths,
    process: Arc<ProcessControl>,
    input_path: PathBuf,
    request: TrimVideoRequest,
) -> Result<VideoResult, String> {
    let progress: Arc<dyn Fn(u8, &str) + Send + Sync> = Arc::new(move |percent, message| {
        emit_progress(&app, percent, message);
    });
    trim_with_progress(paths, process, input_path, request, progress)
}

pub fn render_preview(
    app: AppHandle,
    paths: PortablePaths,
    process: Arc<ProcessControl>,
    input_path: PathBuf,
    request: PreviewVideoRequest,
) -> Result<VideoResult, String> {
    let progress: Arc<dyn Fn(u8, &str) + Send + Sync> = Arc::new(move |percent, message| {
        emit_progress(&app, percent, message);
    });
    render_preview_with_progress(paths, process, input_path, request, progress)
}

fn render_preview_with_progress(
    paths: PortablePaths,
    process: Arc<ProcessControl>,
    input_path: PathBuf,
    request: PreviewVideoRequest,
    progress: Arc<dyn Fn(u8, &str) + Send + Sync>,
) -> Result<VideoResult, String> {
    if process
        .busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("別の動画処理を実行中です。".to_string());
    }
    let _busy_guard = BusyGuard(process.clone());
    if process.shutting_down.load(Ordering::SeqCst) {
        return Err("アプリを終了しています。".to_string());
    }

    let input = validate_input(&input_path.to_string_lossy(), "動画")?;
    let source = inspect_video(&paths, &input.to_string_lossy())?;
    validate_trim_range(
        request.start_seconds,
        request.end_seconds,
        source.duration_seconds,
    )?;
    validate_audio_volume(request.audio_volume)?;
    let source_has_audio = has_audio_stream(&paths, &input)?;
    let added_audio = validate_added_audio(&paths, request.added_audio)?;

    let preview_duration = (request.end_seconds - request.start_seconds).min(10.0);
    let (output_width, output_height) = request.aspect_ratio.preview_dimensions();
    let video_filter = request
        .content_mode
        .video_filter(output_width, output_height);
    let overlay = validate_overlay(request.overlay)?;
    let preview_dir = paths.cache.join("previews");
    fs::create_dir_all(&preview_dir)
        .map_err(|error| format!("プレビューキャッシュを作成できません: {error}"))?;
    let unique_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let preview_output = preview_dir.join(format!(
        "conversion-preview-{}-{unique_id}.mp4",
        std::process::id()
    ));
    let ffmpeg = paths.ffmpeg_path()?;

    paths.log(&format!(
        "video preview started: {:.3}-{:.3} ({}, {}, {})",
        request.start_seconds,
        request.start_seconds + preview_duration,
        request.aspect_ratio.label(),
        request.content_mode.label(),
        overlay_label(overlay.as_ref())
    ));
    progress(2, "保存前プレビューを準備しました。");

    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.3}", request.start_seconds))
        .arg("-i")
        .arg(&input);
    add_overlay_input(&mut command, overlay.as_ref());
    add_added_audio_input(&mut command, added_audio.as_ref());
    let added_audio_input_index = if overlay.is_some() { 2 } else { 1 };
    command.arg("-t").arg(format!("{preview_duration:.3}"));
    add_video_filter_and_mapping(
        &mut command,
        video_filter,
        output_width,
        output_height,
        overlay.as_ref(),
        None,
    );
    add_audio_mapping(
        &mut command,
        source_has_audio,
        request.remove_original_audio,
        request.audio_volume,
        added_audio.as_ref(),
        added_audio_input_index,
    );
    command
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("ultrafast")
        .arg("-crf")
        .arg("28")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("96k")
        .arg("-map_metadata")
        .arg("-1")
        .arg("-metadata:s:v:0")
        .arg("rotate=0")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(&preview_output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .env("TEMP", &paths.temp)
        .env("TMP", &paths.temp);
    hide_console_window(&mut command);

    if let Err(error) = run_trim_process(
        progress.clone(),
        &process,
        command,
        preview_duration,
        "変換プレビューを作成しています…",
    ) {
        let _ = fs::remove_file(&preview_output);
        paths.log(&format!("video preview failed: {error}"));
        return Err(error);
    }

    progress(100, "保存前プレビューを作成しました。");
    paths.log(&format!(
        "video preview completed: {}",
        preview_output.display()
    ));
    inspect_video(&paths, &preview_output.to_string_lossy())
}

fn trim_with_progress(
    paths: PortablePaths,
    process: Arc<ProcessControl>,
    input_path: PathBuf,
    request: TrimVideoRequest,
    progress: Arc<dyn Fn(u8, &str) + Send + Sync>,
) -> Result<VideoResult, String> {
    if process
        .busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("別の動画処理を実行中です。".to_string());
    }
    let _busy_guard = BusyGuard(process.clone());
    if process.shutting_down.load(Ordering::SeqCst) {
        return Err("アプリを終了しています。".to_string());
    }

    let input = validate_input(&input_path.to_string_lossy(), "動画")?;
    let source = inspect_video(&paths, &input.to_string_lossy())?;
    validate_trim_range(
        request.start_seconds,
        request.end_seconds,
        source.duration_seconds,
    )?;
    validate_audio_volume(request.audio_volume)?;
    let source_has_audio = has_audio_stream(&paths, &input)?;
    let added_audio = validate_added_audio(&paths, request.added_audio)?;

    let output = validate_trim_output(&request.output_path, &input)?;
    let ffmpeg = paths.ffmpeg_path()?;
    let session = paths.session_temp()?;
    let temporary_output = session.join("trimmed-output.mp4");
    let trim_duration = request.end_seconds - request.start_seconds;
    let (output_width, output_height) = request.aspect_ratio.dimensions();
    let video_filter = request
        .content_mode
        .video_filter(output_width, output_height);
    let overlay = validate_overlay(request.overlay)?;
    let watermark = validate_watermark(request.watermark, output_width, output_height)?;
    paths.log(&format!(
        "video trim started: {} ({:.3}-{:.3}, {}, {}, {}, {})",
        output.display(),
        request.start_seconds,
        request.end_seconds,
        request.aspect_ratio.label(),
        request.content_mode.label(),
        overlay_label(overlay.as_ref()),
        watermark_label(watermark.as_ref())
    ));
    progress(2, "カット範囲を準備しました。");

    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.3}", request.start_seconds))
        .arg("-i")
        .arg(&input);
    add_overlay_input(&mut command, overlay.as_ref());
    add_watermark_input(&mut command, watermark.as_ref());
    add_added_audio_input(&mut command, added_audio.as_ref());
    let added_audio_input_index =
        1 + usize::from(overlay.is_some()) + usize::from(watermark.is_some());
    command.arg("-t").arg(format!("{trim_duration:.3}"));
    add_video_filter_and_mapping(
        &mut command,
        video_filter,
        output_width,
        output_height,
        overlay.as_ref(),
        watermark.as_ref(),
    );
    add_audio_mapping(
        &mut command,
        source_has_audio,
        request.remove_original_audio,
        request.audio_volume,
        added_audio.as_ref(),
        added_audio_input_index,
    );
    command
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("medium")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("192k")
        .arg("-map_metadata")
        .arg("-1");
    metadata::apply_to_command(&mut command, &request.metadata)?;
    command
        .arg("-metadata:s:v:0")
        .arg("rotate=0")
        .arg("-movflags")
        .arg("+faststart+use_metadata_tags")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(&temporary_output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .env("TEMP", &paths.temp)
        .env("TMP", &paths.temp);
    hide_console_window(&mut command);

    let result = run_trim_process(
        progress.clone(),
        &process,
        command,
        trim_duration,
        "指定範囲をエンコードしています…",
    );
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&session);
        paths.log(&format!("video trim failed: {error}"));
        return Err(error);
    }

    progress(97, "カットした動画を保存しています…");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("保存先フォルダーを作成できません: {error}"))?;
    }
    fs::copy(&temporary_output, &output)
        .map_err(|error| format!("カットした動画を保存できません: {error}"))?;
    let _ = fs::remove_dir_all(&session);
    paths.log(&format!("video trim completed: {}", output.display()));
    progress(100, "カットした動画を保存しました。");
    inspect_video(&paths, &output.to_string_lossy())
}

fn run_trim_process(
    progress: Arc<dyn Fn(u8, &str) + Send + Sync>,
    process: &Arc<ProcessControl>,
    mut command: Command,
    total_seconds: f64,
    progress_message: &'static str,
) -> Result<(), String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("FFmpegを起動できません: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "FFmpegの進捗を取得できません".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "FFmpegの出力を取得できません".to_string())?;
    {
        let mut active = process
            .child
            .lock()
            .map_err(|_| "プロセス管理を開始できません".to_string())?;
        *active = Some(child);
    }

    let error_lines = Arc::new(Mutex::new(VecDeque::<String>::with_capacity(24)));
    let errors_for_stderr = error_lines.clone();
    let stderr_thread = thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut lines) = errors_for_stderr.lock() {
                if lines.len() == 24 {
                    lines.pop_front();
                }
                lines.push_back(line);
            }
        }
    });

    let progress_for_stdout = progress.clone();
    let stdout_thread = thread::spawn(move || {
        let mut last_percent = 0_u8;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(value) = line.strip_prefix("out_time_us=") {
                let elapsed_seconds = value.parse::<u64>().unwrap_or_default() as f64 / 1_000_000.0;
                let percent = (5.0 + elapsed_seconds / total_seconds * 90.0).min(95.0) as u8;
                if percent > last_percent {
                    last_percent = percent;
                    progress_for_stdout(percent, progress_message);
                }
            }
        }
    });

    let exit_status = loop {
        let result = {
            let mut active = process
                .child
                .lock()
                .map_err(|_| "プロセス状態を確認できません".to_string())?;
            match active.as_mut() {
                Some(child) => child.try_wait(),
                None => return Err("動画カットプロセスが中断されました。".to_string()),
            }
        };
        match result {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(format!("FFmpegの状態を確認できません: {error}")),
        }
    };

    if let Ok(mut active) = process.child.lock() {
        active.take();
    }
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    if exit_status.success() {
        return Ok(());
    }

    let details = error_lines
        .lock()
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    Err(format!(
        "FFmpegがカット処理中にエラーを返しました。\n{}",
        compact_error(&details)
    ))
}

fn validate_trim_range(start_seconds: f64, end_seconds: f64, duration: f64) -> Result<(), String> {
    if !start_seconds.is_finite()
        || !end_seconds.is_finite()
        || start_seconds < 0.0
        || end_seconds <= start_seconds
        || end_seconds > duration + 0.05
    {
        return Err(format!(
            "カット範囲は0秒から{duration:.1}秒の間で指定してください。"
        ));
    }
    Ok(())
}

fn validate_audio_volume(volume: u8) -> Result<(), String> {
    if volume > 100 {
        return Err("元の音声は0から100の間で指定してください。".to_string());
    }
    Ok(())
}

fn validate_added_audio(
    paths: &PortablePaths,
    value: Option<AddedAudioSettings>,
) -> Result<Option<ValidatedAddedAudio>, String> {
    value
        .map(|audio| {
            let audio_path = validate_input(&audio.audio_path, "追加する音声")?;
            if !has_audio_stream(paths, &audio_path)? {
                return Err("追加するファイルに音声データがありません。".to_string());
            }
            Ok(ValidatedAddedAudio {
                audio_path,
                loop_audio: audio.loop_audio,
            })
        })
        .transpose()
}

fn add_added_audio_input(command: &mut Command, audio: Option<&ValidatedAddedAudio>) {
    if let Some(audio) = audio {
        if audio.loop_audio {
            command.arg("-stream_loop").arg("-1");
        }
        command.arg("-i").arg(&audio.audio_path);
    }
}

fn add_audio_mapping(
    command: &mut Command,
    source_has_audio: bool,
    remove_original_audio: bool,
    volume: u8,
    added_audio: Option<&ValidatedAddedAudio>,
    added_audio_input_index: usize,
) {
    let keep_original = source_has_audio && !remove_original_audio;
    match (keep_original, added_audio.is_some()) {
        (true, true) => {
            let original_filter = if volume < 100 {
                format!(
                    "[0:a:0]volume={:.2}[original_audio];",
                    f64::from(volume) / 100.0
                )
            } else {
                "[0:a:0]anull[original_audio];".to_string()
            };
            let filter = format!(
                "{original_filter}[original_audio][{added_audio_input_index}:a:0]amix=inputs=2:duration=longest:dropout_transition=0:normalize=0,alimiter=limit=0.95[outa]"
            );
            command
                .arg("-filter_complex")
                .arg(filter)
                .arg("-map")
                .arg("[outa]");
        }
        (true, false) => {
            command.arg("-map").arg("0:a:0");
            if volume < 100 {
                command
                    .arg("-af")
                    .arg(format!("volume={:.2}", f64::from(volume) / 100.0));
            }
        }
        (false, true) => {
            command
                .arg("-map")
                .arg(format!("{added_audio_input_index}:a:0"));
        }
        (false, false) => {}
    }
}

fn has_audio_stream(paths: &PortablePaths, input: &Path) -> Result<bool, String> {
    let mut command = Command::new(paths.ffmpeg_path()?);
    command
        .arg("-hide_banner")
        .arg("-i")
        .arg(input)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    hide_console_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("音声情報を取得できません: {error}"))?;
    Ok(String::from_utf8_lossy(&output.stderr).contains("Audio:"))
}

fn validate_overlay(value: Option<OverlaySettings>) -> Result<Option<ValidatedOverlay>, String> {
    value
        .map(|overlay| {
            Ok(ValidatedOverlay {
                image_path: validate_input(&overlay.image_path, "重ねる画像")?,
                scale: overlay.scale,
                position: overlay.position,
                background: overlay.background,
            })
        })
        .transpose()
}

fn validate_watermark(
    value: Option<WatermarkSettings>,
    output_width: u32,
    output_height: u32,
) -> Result<Option<ValidatedWatermark>, String> {
    value
        .map(|watermark| {
            if watermark.opacity > 100 {
                return Err(
                    "ウォーターマークの透過率は0から100の間で指定してください。".to_string()
                );
            }
            if !(-180..=180).contains(&watermark.angle) {
                return Err(
                    "ウォーターマークの角度は-180度から180度の間で指定してください。".to_string(),
                );
            }
            if !(1..=50).contains(&watermark.count) {
                return Err("ウォーターマークの個数は1から50の間で指定してください。".to_string());
            }
            if watermark.spacing > output_width.max(output_height) {
                return Err("ウォーターマークの間隔が動画サイズを超えています。".to_string());
            }
            if watermark.x > output_width || watermark.y > output_height {
                return Err("ウォーターマークの位置が動画サイズを超えています。".to_string());
            }
            Ok(ValidatedWatermark {
                image_path: validate_input(&watermark.image_path, "ウォーターマーク")?,
                scale: watermark.scale,
                position: watermark.position,
                x: watermark.x,
                y: watermark.y,
                opacity: watermark.opacity,
                spacing: watermark.spacing,
                angle: watermark.angle,
                count: watermark.count,
            })
        })
        .transpose()
}

fn add_overlay_input(command: &mut Command, overlay: Option<&ValidatedOverlay>) {
    if let Some(overlay) = overlay {
        command
            .arg("-loop")
            .arg("1")
            .arg("-i")
            .arg(&overlay.image_path);
    }
}

fn add_watermark_input(command: &mut Command, watermark: Option<&ValidatedWatermark>) {
    if let Some(watermark) = watermark {
        command
            .arg("-loop")
            .arg("1")
            .arg("-i")
            .arg(&watermark.image_path);
    }
}

fn add_video_filter_and_mapping(
    command: &mut Command,
    base_filter: String,
    output_width: u32,
    output_height: u32,
    overlay: Option<&ValidatedOverlay>,
    watermark: Option<&ValidatedWatermark>,
) {
    if overlay.is_none() && watermark.is_none() {
        command.arg("-map").arg("0:v:0").arg("-vf").arg(base_filter);
        return;
    }

    let filter = build_layered_video_filter(
        &base_filter,
        output_width,
        output_height,
        overlay,
        watermark,
    );
    command
        .arg("-filter_complex")
        .arg(filter)
        .arg("-map")
        .arg("[outv]");
}

fn build_layered_video_filter(
    base_filter: &str,
    output_width: u32,
    output_height: u32,
    overlay: Option<&ValidatedOverlay>,
    watermark: Option<&ValidatedWatermark>,
) -> String {
    let mut filter = format!("[0:v]{base_filter}[base]");
    let mut current = "base".to_string();

    if let Some(overlay) = overlay {
        let percent = overlay.scale.percent();
        let box_width = (output_width * percent / 100).max(2);
        let box_height = (output_height * percent / 100).max(2);
        let coordinates = overlay
            .position
            .coordinates((output_width / 40).max(8), (output_height / 40).max(8));
        match overlay.background.color() {
            None => filter.push_str(&format!(
                ";[1:v]format=rgba,scale={box_width}:{box_height}:force_original_aspect_ratio=decrease[overlay_image];[{current}][overlay_image]overlay={coordinates}:format=auto[after_overlay]"
            )),
            Some(color) => filter.push_str(&format!(
                ";[1:v]format=rgba,scale={box_width}:{box_height}:force_original_aspect_ratio=decrease[overlay_image];color=c={color}:s={box_width}x{box_height}:r=30,format=rgba[overlay_plate];[overlay_plate][overlay_image]overlay=(W-w)/2:(H-h)/2:format=auto[composed_overlay];[{current}][composed_overlay]overlay={coordinates}:format=auto[after_overlay]"
            )),
        }
        current = "after_overlay".to_string();
    }

    if let Some(watermark) = watermark {
        let watermark_input = if overlay.is_some() { 2 } else { 1 };
        let percent = watermark.scale.percent();
        let box_width = (output_width * percent / 100).max(2);
        let box_height = (output_height * percent / 100).max(2);
        let opacity = f64::from(watermark.opacity) / 100.0;
        let radians = f64::from(watermark.angle).to_radians();
        let labels = (0..watermark.count)
            .map(|index| format!("[watermark_{index}]"))
            .collect::<String>();
        let split = if watermark.count == 1 {
            "[watermark_0]".to_string()
        } else {
            format!(",split={}{labels}", watermark.count)
        };
        filter.push_str(&format!(
            ";[{watermark_input}:v]format=rgba,scale={box_width}:{box_height}:force_original_aspect_ratio=decrease,colorchannelmixer=aa={opacity:.2},rotate={radians:.6}:ow=rotw(iw):oh=roth(ih):c=none{split}"
        ));

        let positions = watermark_positions(watermark, output_width, output_height);
        for (index, (x, y)) in positions.into_iter().enumerate() {
            let next = if index + 1 == usize::from(watermark.count) {
                "watermarked".to_string()
            } else {
                format!("watermark_layer_{index}")
            };
            filter.push_str(&format!(
                ";[{current}][watermark_{index}]overlay={x}:{y}:format=auto[{next}]"
            ));
            current = next;
        }
    }

    filter.push_str(&format!(";[{current}]format=yuv420p[outv]"));
    filter
}

fn watermark_positions(
    watermark: &ValidatedWatermark,
    output_width: u32,
    output_height: u32,
) -> Vec<(u32, u32)> {
    let percent = watermark.scale.percent();
    let box_width = (output_width * percent / 100).max(2);
    let box_height = (output_height * percent / 100).max(2);
    let max_x = output_width.saturating_sub(box_width);
    let max_y = output_height.saturating_sub(box_height);
    let base_x = watermark.x.min(max_x);
    let base_y = watermark.y.min(max_y);
    let step_x = u64::from(box_width.saturating_add(watermark.spacing).max(1));
    let step_y = u64::from(box_height.saturating_add(watermark.spacing).max(1));
    let span_x = u64::from(max_x) + 1;
    let span_y = u64::from(max_y) + 1;

    (0..watermark.count)
        .map(|index| {
            let index = u64::from(index);
            (
                ((u64::from(base_x) + index * step_x) % span_x) as u32,
                ((u64::from(base_y) + index * step_y) % span_y) as u32,
            )
        })
        .collect()
}

fn overlay_label(overlay: Option<&ValidatedOverlay>) -> String {
    overlay
        .map(|value| {
            format!(
                "overlay:{}:{}:{}",
                value.scale.label(),
                value.position.label(),
                value.background.label()
            )
        })
        .unwrap_or_else(|| "no-overlay".to_string())
}

fn watermark_label(watermark: Option<&ValidatedWatermark>) -> String {
    watermark
        .map(|value| {
            format!(
                "watermark:{}:{}:{}:{}:{}:{}:{}",
                value.scale.label(),
                value.position.label(),
                value.x,
                value.y,
                value.opacity,
                value.spacing,
                value.count
            )
        })
        .unwrap_or_else(|| "no-watermark".to_string())
}
fn validate_input(value: &str, label: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_file() {
        return Err(format!(
            "{label}ファイルが見つかりません: {}",
            path.display()
        ));
    }
    fs::canonicalize(&path).map_err(|error| format!("{label}ファイルを読み取れません: {error}"))
}

fn validate_output(value: &str, audio: &Path, image: &Path) -> Result<PathBuf, String> {
    let mut path = PathBuf::from(value);
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("mp4"))
        != Some(true)
    {
        path.set_extension("mp4");
    }
    if path.as_os_str().is_empty() {
        return Err("保存先を選択してください。".to_string());
    }
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|error| format!("保存先を解決できません: {error}"))?
            .join(path)
    };
    if absolute == audio || absolute == image {
        return Err("入力ファイルと同じ場所には保存できません。".to_string());
    }
    Ok(absolute)
}

fn validate_trim_output(value: &str, input: &Path) -> Result<PathBuf, String> {
    let mut path = PathBuf::from(value);
    if path.as_os_str().is_empty() {
        return Err("保存先を選択してください。".to_string());
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("mp4"))
        != Some(true)
    {
        path.set_extension("mp4");
    }
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|error| format!("保存先を解決できません: {error}"))?
            .join(path)
    };
    let same_file = if absolute.exists() {
        fs::canonicalize(&absolute).ok().as_deref() == fs::canonicalize(input).ok().as_deref()
    } else {
        absolute
            .to_string_lossy()
            .eq_ignore_ascii_case(&input.to_string_lossy())
    };
    if same_file {
        return Err("元の動画とは別のファイル名で保存してください。".to_string());
    }
    Ok(absolute)
}

fn parse_duration_seconds(details: &str) -> Option<f64> {
    let pattern = Regex::new(r"Duration: (\d{2}):(\d{2}):(\d{2}(?:\.\d+)?)").ok()?;
    let captures = pattern.captures(details)?;
    let hours = captures[1].parse::<f64>().ok()?;
    let minutes = captures[2].parse::<f64>().ok()?;
    let seconds = captures[3].parse::<f64>().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn parse_video_dimensions(details: &str) -> Option<(u32, u32)> {
    let dimensions_pattern = Regex::new(r"Video:[^\r\n]*?[, ](\d{2,5})x(\d{2,5})(?:[,\s])").ok()?;
    let captures = dimensions_pattern.captures(details)?;
    let mut width = captures[1].parse::<u32>().ok()?;
    let mut height = captures[2].parse::<u32>().ok()?;

    let rotation = Regex::new(r"rotation of\s+(-?\d+(?:\.\d+)?)")
        .ok()
        .and_then(|pattern| pattern.captures(details))
        .and_then(|captures| captures[1].parse::<f64>().ok())
        .unwrap_or_default()
        .abs()
        % 180.0;
    if (45.0..135.0).contains(&rotation) {
        std::mem::swap(&mut width, &mut height);
    }
    Some((width, height))
}

fn hide_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn emit_progress(app: &AppHandle, percent: u8, message: &str) {
    let _ = app.emit(
        "video-progress",
        ProgressPayload {
            percent,
            message: message.to_string(),
        },
    );
}

fn compact_error(details: &str) -> String {
    let useful = details
        .lines()
        .rev()
        .find(|line| line.contains("Error") || line.contains("Invalid") || line.contains("failed"))
        .unwrap_or("入力ファイルの形式または保存先を確認してください。");
    useful.chars().take(300).collect()
}

pub fn stop_active_process(process: &ProcessControl) {
    process.shutting_down.store(true, Ordering::SeqCst);
    if let Ok(mut active) = process.child.lock() {
        if let Some(child) = active.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        active.take();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compact_error, has_audio_stream, inspect_video, parse_duration_seconds,
        parse_video_dimensions, render_preview_with_progress, trim_with_progress,
        validate_audio_volume, watermark_positions, AddedAudioSettings, ContentMode,
        CreateOutputSize, OutputAspectRatio, OverlayBackground, OverlayPosition, OverlayScale,
        OverlaySettings, PreviewVideoRequest, TrimVideoRequest, ValidatedWatermark,
        WatermarkSettings,
    };
    use crate::{metadata::EditableMetadata, portable::PortablePaths, state::ProcessControl};
    use std::{
        fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::Arc,
    };

    fn mean_volume(ffmpeg: &Path, input: &Path) -> f64 {
        let output = Command::new(ffmpeg)
            .arg("-hide_banner")
            .arg("-i")
            .arg(input)
            .args(["-af", "volumedetect", "-f", "null", "-"])
            .stdout(Stdio::null())
            .output()
            .expect("measure audio volume");
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .find_map(|line| {
                line.split_once("mean_volume:")
                    .and_then(|(_, value)| value.trim().split_whitespace().next())
                    .and_then(|value| value.parse::<f64>().ok())
            })
            .expect("mean volume")
    }

    #[test]
    fn chooses_a_useful_ffmpeg_error() {
        let text = "metadata\nInvalid data found when processing input\ntrailer";
        assert!(compact_error(text).contains("Invalid data"));
    }

    #[test]
    fn parses_ffmpeg_duration() {
        let text = "Duration: 01:02:03.50, start: 0.000000, bitrate: 123 kb/s";
        assert_eq!(parse_duration_seconds(text), Some(3723.5));
    }

    #[test]
    fn parses_dimensions_and_phone_rotation() {
        let landscape = "Stream #0:0: Video: h264 (High), yuv420p, 1920x1080, 30 fps";
        assert_eq!(parse_video_dimensions(landscape), Some((1920, 1080)));

        let rotated =
            "Stream #0:0: Video: h264, yuv420p, 1920x1080, 30 fps\nrotation of -90.00 degrees";
        assert_eq!(parse_video_dimensions(rotated), Some((1080, 1920)));
        assert_eq!(OutputAspectRatio::Landscape.dimensions(), (1920, 1080));
        assert_eq!(OutputAspectRatio::Portrait.dimensions(), (1080, 1920));
        assert_eq!(OutputAspectRatio::Portrait.preview_dimensions(), (360, 640));
        assert!(ContentMode::Contain
            .video_filter(1080, 1920)
            .contains("force_original_aspect_ratio=decrease,pad=1080:1920"));
        assert!(ContentMode::Cover
            .video_filter(1080, 1920)
            .contains("force_original_aspect_ratio=increase,crop=1080:1920"));
        assert!(CreateOutputSize::Landscape
            .video_filter()
            .contains("1920:1080"));
        assert!(CreateOutputSize::Portrait
            .video_filter()
            .contains("1080:1920"));
        assert!(CreateOutputSize::Image
            .video_filter()
            .contains("trunc(iw/2)*2:trunc(ih/2)*2"));
        assert!(validate_audio_volume(0).is_ok());
        assert!(validate_audio_volume(100).is_ok());
        assert!(validate_audio_volume(101).is_err());
    }

    #[test]
    fn places_repeated_watermarks_inside_the_video() {
        let watermark = ValidatedWatermark {
            image_path: PathBuf::new(),
            scale: OverlayScale::Small,
            position: OverlayPosition::BottomRight,
            x: 100,
            y: 200,
            opacity: 50,
            spacing: 48,
            angle: 15,
            count: 10,
        };
        let positions = watermark_positions(&watermark, 1080, 1920);
        assert_eq!(positions.len(), 10);
        assert_eq!(positions[0], (100, 200));
        assert!(positions.iter().all(|(x, y)| *x <= 864 && *y <= 1536));
    }

    #[test]
    fn imports_and_trims_a_real_video() {
        let paths = PortablePaths::initialize().expect("portable paths");
        let test_dir = paths.temp.join(format!("trim-test-{}", std::process::id()));
        fs::create_dir_all(&test_dir).expect("test directory");
        let source = test_dir.join("source.mp4");
        let trimmed = test_dir.join("trimmed.mp4");
        let overlay_image = test_dir.join("overlay.bmp");
        let still_image = test_dir.join("still.bmp");
        let image_sized_video = test_dir.join("image-sized.mp4");
        let added_audio = test_dir.join("added.wav");
        let ffmpeg = paths.ffmpeg_path().expect("embedded ffmpeg");

        let status = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg("color=c=blue:s=320x240:r=30:d=3")
            .args(["-f", "lavfi", "-i"])
            .arg("sine=frequency=440:duration=3")
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-metadata",
                "title=Original Title",
                "-metadata",
                "artist=Original Artist",
                "-metadata",
                "remove_me=delete this",
                "-movflags",
                "+faststart+use_metadata_tags",
                "-shortest",
            ])
            .arg(&source)
            .status()
            .expect("create source video");
        assert!(status.success());

        let overlay_status = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=80x60",
                "-frames:v",
                "1",
                "-update",
                "1",
            ])
            .arg(&overlay_image)
            .status()
            .expect("create overlay image");
        assert!(overlay_status.success());

        let still_status = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=green:s=322x242",
                "-frames:v",
                "1",
                "-update",
                "1",
            ])
            .arg(&still_image)
            .status()
            .expect("create still image");
        assert!(still_status.success());

        let image_size_status = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-loop",
                "1",
                "-i",
            ])
            .arg(&still_image)
            .args(["-f", "lavfi", "-i", "sine=frequency=330:duration=1", "-vf"])
            .arg(CreateOutputSize::Image.video_filter())
            .args(["-c:v", "libx264", "-c:a", "aac", "-shortest"])
            .arg(&image_sized_video)
            .status()
            .expect("create image-sized video");
        assert!(image_size_status.success());
        let image_sized = inspect_video(&paths, &image_sized_video.to_string_lossy())
            .expect("inspect image-sized video");
        assert_eq!((image_sized.width, image_sized.height), (322, 242));

        let added_audio_status = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=660:duration=0.4",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&added_audio)
            .status()
            .expect("create added audio");
        assert!(added_audio_status.success());

        let imported =
            inspect_video(&paths, &source.to_string_lossy()).expect("inspect imported video");
        assert!((2.9..=3.1).contains(&imported.duration_seconds));
        assert_eq!((imported.width, imported.height), (320, 240));
        assert_eq!(imported.metadata.technical.frame_rate, "30 fps");
        assert_eq!(imported.metadata.technical.video_codec, "H.264");
        assert_eq!(imported.metadata.technical.audio_codec, "AAC");
        assert_eq!(imported.metadata.editable.title, "Original Title");
        assert_eq!(imported.metadata.editable.artist_author, "Original Artist");
        assert!(imported
            .metadata
            .editable
            .custom_metadata
            .contains("remove_me=delete this"));

        let silent_preview = render_preview_with_progress(
            paths.clone(),
            Arc::new(ProcessControl::default()),
            source.clone(),
            PreviewVideoRequest {
                start_seconds: 0.25,
                end_seconds: 3.0,
                aspect_ratio: OutputAspectRatio::Portrait,
                content_mode: ContentMode::Cover,
                overlay: None,
                audio_volume: 100,
                remove_original_audio: true,
                added_audio: None,
            },
            Arc::new(|_, _| {}),
        )
        .expect("render preview without original audio");
        assert!(
            !has_audio_stream(&paths, &PathBuf::from(&silent_preview.output_path))
                .expect("inspect silent preview")
        );

        let preview = render_preview_with_progress(
            paths.clone(),
            Arc::new(ProcessControl::default()),
            source.clone(),
            PreviewVideoRequest {
                start_seconds: 0.25,
                end_seconds: 3.0,
                aspect_ratio: OutputAspectRatio::Portrait,
                content_mode: ContentMode::Cover,
                overlay: Some(OverlaySettings {
                    image_path: overlay_image.to_string_lossy().into_owned(),
                    scale: OverlayScale::Medium,
                    position: OverlayPosition::BottomRight,
                    background: OverlayBackground::Black,
                }),
                audio_volume: 100,
                remove_original_audio: false,
                added_audio: Some(AddedAudioSettings {
                    audio_path: added_audio.to_string_lossy().into_owned(),
                    loop_audio: true,
                }),
            },
            Arc::new(|_, _| {}),
        )
        .expect("render preview with looped replacement audio");
        assert!((2.6..=2.9).contains(&preview.duration_seconds));
        assert_eq!((preview.width, preview.height), (360, 640));
        assert!(PathBuf::from(&preview.output_path).is_file());
        assert!(
            has_audio_stream(&paths, &PathBuf::from(&preview.output_path))
                .expect("inspect replacement audio")
        );

        let result = trim_with_progress(
            paths.clone(),
            Arc::new(ProcessControl::default()),
            source.clone(),
            TrimVideoRequest {
                output_path: trimmed.to_string_lossy().into_owned(),
                start_seconds: 0.5,
                end_seconds: 1.7,
                aspect_ratio: OutputAspectRatio::Portrait,
                content_mode: ContentMode::Cover,
                overlay: Some(OverlaySettings {
                    image_path: overlay_image.to_string_lossy().into_owned(),
                    scale: OverlayScale::Medium,
                    position: OverlayPosition::BottomRight,
                    background: OverlayBackground::Black,
                }),
                watermark: Some(WatermarkSettings {
                    image_path: overlay_image.to_string_lossy().into_owned(),
                    scale: OverlayScale::Small,
                    position: OverlayPosition::TopLeft,
                    x: 24,
                    y: 48,
                    opacity: 60,
                    spacing: 48,
                    angle: 15,
                    count: 3,
                }),
                metadata: EditableMetadata {
                    title: "Edited Title".to_string(),
                    encoded_by: "QuickVideoMaker".to_string(),
                    software: "QuickVideoMaker".to_string(),
                    version: "1.6.0".to_string(),
                    handler_name: "QuickVideoMaker Video".to_string(),
                    custom_metadata: "custom_key=Custom Value".to_string(),
                    xmp: "<xmp>QuickVideoMaker</xmp>".to_string(),
                    ..EditableMetadata::default()
                },
                audio_volume: 40,
                remove_original_audio: false,
                added_audio: None,
            },
            Arc::new(|_, _| {}),
        )
        .expect("trim video");
        assert!((1.1..=1.3).contains(&result.duration_seconds));
        assert_eq!((result.width, result.height), (1080, 1920));
        assert!(trimmed.is_file());
        assert_eq!(result.metadata.editable.title, "Edited Title");
        assert!(result.metadata.editable.artist_author.is_empty());
        assert!(result
            .metadata
            .editable
            .custom_metadata
            .contains("custom_key=Custom Value"));
        assert!(!result
            .metadata
            .editable
            .custom_metadata
            .contains("remove_me"));
        assert_eq!(
            result.metadata.editable.handler_name,
            "QuickVideoMaker Video"
        );
        let source_volume = mean_volume(&ffmpeg, &source);
        let trimmed_volume = mean_volume(&ffmpeg, &trimmed);
        assert!(trimmed_volume < source_volume - 6.0);
        assert!(trimmed_volume > source_volume - 10.0);
        let _ = fs::remove_dir_all(&test_dir);
        paths.clean_preview_cache().expect("clean preview cache");
    }
}
