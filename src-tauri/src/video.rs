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
    time::Duration,
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
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoResult {
    pub output_path: String,
    pub duration_seconds: f64,
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

    Ok(VideoResult {
        output_path: input.to_string_lossy().into_owned(),
        duration_seconds,
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
    if !request.start_seconds.is_finite()
        || !request.end_seconds.is_finite()
        || request.start_seconds < 0.0
        || request.end_seconds <= request.start_seconds
        || request.end_seconds > source.duration_seconds + 0.05
    {
        return Err(format!(
            "カット範囲は0秒から{:.1}秒の間で指定してください。",
            source.duration_seconds
        ));
    }

    let output = validate_trim_output(&request.output_path, &input)?;
    let ffmpeg = paths.ffmpeg_path()?;
    let session = paths.session_temp()?;
    let temporary_output = session.join("trimmed-output.mp4");
    let trim_duration = request.end_seconds - request.start_seconds;
    paths.log(&format!(
        "video trim started: {} ({:.3}-{:.3})",
        output.display(),
        request.start_seconds,
        request.end_seconds
    ));
    progress(2, "カット範囲を準備しました。");

    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{:.3}", request.start_seconds))
        .arg("-i")
        .arg(&input)
        .arg("-t")
        .arg(format!("{trim_duration:.3}"))
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("0:a:0?")
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("medium")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("192k")
        .arg("-pix_fmt")
        .arg("yuv420p")
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

    let result = run_trim_process(progress.clone(), &process, command, trim_duration);
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
                    progress_for_stdout(percent, "指定範囲をエンコードしています…");
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
        compact_error, inspect_video, parse_duration_seconds, trim_with_progress, TrimVideoRequest,
    };
    use crate::{portable::PortablePaths, state::ProcessControl};
    use std::{fs, process::Command, sync::Arc};

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
    fn imports_and_trims_a_real_video() {
        let paths = PortablePaths::initialize().expect("portable paths");
        let test_dir = paths.temp.join(format!("trim-test-{}", std::process::id()));
        fs::create_dir_all(&test_dir).expect("test directory");
        let source = test_dir.join("source.mp4");
        let trimmed = test_dir.join("trimmed.mp4");
        let ffmpeg = paths.ffmpeg_path().expect("embedded ffmpeg");

        let status = Command::new(ffmpeg)
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

        let imported =
            inspect_video(&paths, &source.to_string_lossy()).expect("inspect imported video");
        assert!((2.9..=3.1).contains(&imported.duration_seconds));

        let result = trim_with_progress(
            paths.clone(),
            Arc::new(ProcessControl::default()),
            source,
            TrimVideoRequest {
                output_path: trimmed.to_string_lossy().into_owned(),
                start_seconds: 0.5,
                end_seconds: 1.7,
            },
            Arc::new(|_, _| {}),
        )
        .expect("trim video");
        assert!((1.1..=1.3).contains(&result.duration_seconds));
        assert!(trimmed.is_file());
        let _ = fs::remove_dir_all(&test_dir);
    }
}
