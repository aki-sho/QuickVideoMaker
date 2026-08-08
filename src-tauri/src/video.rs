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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewVideoRequest {
    start_seconds: f64,
    end_seconds: f64,
    aspect_ratio: OutputAspectRatio,
    content_mode: ContentMode,
    overlay: Option<OverlaySettings>,
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
    paths.log(&format!("video creation started: {}", output.display()));

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
        .arg("scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,format=yuv420p")
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

    Ok(VideoResult {
        output_path: input.to_string_lossy().into_owned(),
        duration_seconds,
        width,
        height,
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
    command.arg("-t").arg(format!("{preview_duration:.3}"));
    add_video_filter_and_mapping(
        &mut command,
        video_filter,
        output_width,
        output_height,
        overlay.as_ref(),
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
    paths.log(&format!(
        "video trim started: {} ({:.3}-{:.3}, {}, {}, {})",
        output.display(),
        request.start_seconds,
        request.end_seconds,
        request.aspect_ratio.label(),
        request.content_mode.label(),
        overlay_label(overlay.as_ref())
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
    command.arg("-t").arg(format!("{trim_duration:.3}"));
    add_video_filter_and_mapping(
        &mut command,
        video_filter,
        output_width,
        output_height,
        overlay.as_ref(),
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
        .arg("-1")
        .arg("-metadata:s:v:0")
        .arg("rotate=0")
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

fn add_overlay_input(command: &mut Command, overlay: Option<&ValidatedOverlay>) {
    if let Some(overlay) = overlay {
        command
            .arg("-loop")
            .arg("1")
            .arg("-i")
            .arg(&overlay.image_path);
    }
}

fn add_video_filter_and_mapping(
    command: &mut Command,
    base_filter: String,
    output_width: u32,
    output_height: u32,
    overlay: Option<&ValidatedOverlay>,
) {
    if let Some(overlay) = overlay {
        let percent = overlay.scale.percent();
        let box_width = output_width * percent / 100;
        let box_height = output_height * percent / 100;
        let coordinates = overlay
            .position
            .coordinates((output_width / 40).max(8), (output_height / 40).max(8));
        let filter = build_overlay_filter(
            &base_filter,
            box_width,
            box_height,
            &coordinates,
            overlay.background,
        );
        command
            .arg("-filter_complex")
            .arg(filter)
            .arg("-map")
            .arg("[outv]");
    } else {
        command.arg("-map").arg("0:v:0").arg("-vf").arg(base_filter);
    }
    command.arg("-map").arg("0:a:0?");
}

fn build_overlay_filter(
    base_filter: &str,
    box_width: u32,
    box_height: u32,
    coordinates: &str,
    background: OverlayBackground,
) -> String {
    match background.color() {
        None => format!(
            "[0:v]{base_filter}[base];[1:v]format=rgba,scale={box_width}:{box_height}:force_original_aspect_ratio=decrease[overlay];[base][overlay]overlay={coordinates}:format=auto,format=yuv420p[outv]"
        ),
        Some(color) => format!(
            "[0:v]{base_filter}[base];[1:v]format=rgba,scale={box_width}:{box_height}:force_original_aspect_ratio=decrease[overlay_image];color=c={color}:s={box_width}x{box_height}:r=30,format=rgba[plate];[plate][overlay_image]overlay=(W-w)/2:(H-h)/2:format=auto[overlay];[base][overlay]overlay={coordinates}:format=auto,format=yuv420p[outv]"
        ),
    }
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
        compact_error, inspect_video, parse_duration_seconds, parse_video_dimensions,
        render_preview_with_progress, trim_with_progress, ContentMode, OutputAspectRatio,
        OverlayBackground, OverlayPosition, OverlayScale, OverlaySettings, PreviewVideoRequest,
        TrimVideoRequest,
    };
    use crate::{portable::PortablePaths, state::ProcessControl};
    use std::{fs, path::PathBuf, process::Command, sync::Arc};

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
    }

    #[test]
    fn imports_and_trims_a_real_video() {
        let paths = PortablePaths::initialize().expect("portable paths");
        let test_dir = paths.temp.join(format!("trim-test-{}", std::process::id()));
        fs::create_dir_all(&test_dir).expect("test directory");
        let source = test_dir.join("source.mp4");
        let trimmed = test_dir.join("trimmed.mp4");
        let overlay_image = test_dir.join("overlay.bmp");
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

        let imported =
            inspect_video(&paths, &source.to_string_lossy()).expect("inspect imported video");
        assert!((2.9..=3.1).contains(&imported.duration_seconds));
        assert_eq!((imported.width, imported.height), (320, 240));

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
            },
            Arc::new(|_, _| {}),
        )
        .expect("render preview");
        assert!((2.6..=2.9).contains(&preview.duration_seconds));
        assert_eq!((preview.width, preview.height), (360, 640));
        assert!(PathBuf::from(&preview.output_path).is_file());

        let result = trim_with_progress(
            paths.clone(),
            Arc::new(ProcessControl::default()),
            source,
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
            },
            Arc::new(|_, _| {}),
        )
        .expect("trim video");
        assert!((1.1..=1.3).contains(&result.duration_seconds));
        assert_eq!((result.width, result.height), (1080, 1920));
        assert!(trimmed.is_file());
        let _ = fs::remove_dir_all(&test_dir);
        paths.clean_preview_cache().expect("clean preview cache");
    }
}
