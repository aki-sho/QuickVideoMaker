mod c2pa_metadata;
mod dialogs;
mod metadata;
mod portable;
mod preview;
mod state;
mod video;

use state::{AppState, ProcessControl};
use std::sync::Arc;
use std::{path::PathBuf, sync::Mutex};
use tauri::Manager;

#[tauri::command]
async fn create_video(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: video::CreateVideoRequest,
) -> Result<video::VideoResult, String> {
    let paths = state.paths.clone();
    let process = state.process.clone();
    let preview = state.preview.clone();
    let source_video = state.source_video.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || video::generate(app, paths, process, request))
            .await
            .map_err(|error| format!("動画作成タスクが停止しました: {error}"))??;
    set_source_and_preview(&source_video, &preview, &result.output_path)?;
    Ok(result)
}

#[tauri::command]
async fn import_video(
    state: tauri::State<'_, AppState>,
    video_path: String,
) -> Result<video::VideoResult, String> {
    let paths = state.paths.clone();
    let preview = state.preview.clone();
    let source_video = state.source_video.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || video::inspect_video(&paths, &video_path))
            .await
            .map_err(|error| format!("動画情報の取得タスクが停止しました: {error}"))??;
    set_source_and_preview(&source_video, &preview, &result.output_path)?;
    Ok(result)
}

#[tauri::command]
async fn trim_video(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: video::TrimVideoRequest,
) -> Result<video::VideoResult, String> {
    let input = state
        .source_video
        .lock()
        .map_err(|_| "プレビュー動画の状態を取得できません".to_string())?
        .clone()
        .ok_or_else(|| "先に動画を作成またはインポートしてください。".to_string())?;
    let paths = state.paths.clone();
    let process = state.process.clone();
    let preview = state.preview.clone();
    let source_video = state.source_video.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        video::trim(app, paths, process, input, request)
    })
    .await
    .map_err(|error| format!("動画カットタスクが停止しました: {error}"))??;
    set_source_and_preview(&source_video, &preview, &result.output_path)?;
    Ok(result)
}

#[tauri::command]
async fn render_video_preview(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: video::PreviewVideoRequest,
) -> Result<video::VideoResult, String> {
    let input = state
        .source_video
        .lock()
        .map_err(|_| "元動画の状態を取得できません".to_string())?
        .clone()
        .ok_or_else(|| "先に動画を作成またはインポートしてください。".to_string())?;
    let paths = state.paths.clone();
    let process = state.process.clone();
    let preview = state.preview.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        video::render_preview(app, paths, process, input, request)
    })
    .await
    .map_err(|error| format!("変換プレビュータスクが停止しました: {error}"))??;
    set_preview(&preview, &result.output_path)?;
    Ok(result)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ImagePreviewData {
    bytes: Vec<u8>,
    mime_type: String,
}

#[tauri::command]
fn load_image_preview(image_path: String) -> Result<ImagePreviewData, String> {
    let path = PathBuf::from(&image_path);
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("画像プレビューを読み込めません: {error}"))?;
    if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
        return Err("画像プレビューは32MB以下の画像ファイルを選択してください。".to_string());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "画像の拡張子を確認してください。".to_string())?;
    let mime_type = match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        _ => return Err("対応していない画像形式です。".to_string()),
    };
    let bytes =
        std::fs::read(path).map_err(|error| format!("画像プレビューを読み込めません: {error}"))?;
    Ok(ImagePreviewData {
        bytes,
        mime_type: mime_type.to_string(),
    })
}

#[tauri::command]
async fn inspect_c2pa(
    state: tauri::State<'_, AppState>,
) -> Result<c2pa_metadata::C2paDetails, String> {
    let input = state
        .source_video
        .lock()
        .map_err(|_| "元動画の状態を取得できません".to_string())?
        .clone()
        .ok_or_else(|| "先に動画を作成またはインポートしてください。".to_string())?;
    tauri::async_runtime::spawn_blocking(move || c2pa_metadata::inspect(&input))
        .await
        .map_err(|error| format!("証明情報の検証タスクが停止しました: {error}"))
}

fn set_preview(preview: &preview::PreviewStore, path: &str) -> Result<(), String> {
    let mut value = preview
        .lock()
        .map_err(|_| "プレビュー動画を設定できません".to_string())?;
    *value = Some(PathBuf::from(path));
    Ok(())
}

fn set_source_and_preview(
    source_video: &preview::PreviewStore,
    preview: &preview::PreviewStore,
    path: &str,
) -> Result<(), String> {
    set_preview(source_video, path)?;
    set_preview(preview, path)
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let paths = portable::PortablePaths::initialize().map_err(std::io::Error::other)?;
    let process = Arc::new(ProcessControl::default());
    let source_video = Arc::new(Mutex::new(None));
    let preview = Arc::new(Mutex::new(None));
    let preview_for_protocol = preview.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .register_uri_scheme_protocol("qvm", move |_context, request| {
            preview::response(&request, &preview_for_protocol)
        })
        .manage(AppState {
            paths: paths.clone(),
            process: process.clone(),
            source_video,
            preview,
        })
        .invoke_handler(tauri::generate_handler![
            dialogs::select_music_file,
            dialogs::select_image_file,
            dialogs::select_video_file,
            dialogs::select_output_file,
            create_video,
            import_video,
            trim_video,
            render_video_preview,
            load_image_preview,
            inspect_c2pa
        ])
        .build(tauri::generate_context!())?;

    app.run(move |_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            video::stop_active_process(&process);
            let _ = paths.clean_temp();
            let _ = paths.clean_preview_cache();
            paths.log("application shutdown");
        }
    });

    Ok(())
}
