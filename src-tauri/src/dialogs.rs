use rfd::FileDialog;

#[tauri::command]
pub fn select_music_file() -> Option<String> {
    FileDialog::new()
        .set_title("音楽ファイルを選択")
        .add_filter(
            "音楽ファイル",
            &["mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "wma"],
        )
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn select_image_file() -> Option<String> {
    FileDialog::new()
        .set_title("画像ファイルを選択")
        .add_filter(
            "画像ファイル",
            &["jpg", "jpeg", "png", "bmp", "webp", "gif", "tif", "tiff"],
        )
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn select_video_file() -> Option<String> {
    FileDialog::new()
        .set_title("カットする動画を選択")
        .add_filter(
            "動画ファイル",
            &["mp4", "mov", "m4v", "avi", "mkv", "webm", "wmv"],
        )
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn select_output_file(suggested_name: Option<String>) -> Option<String> {
    FileDialog::new()
        .set_title("MP4動画の保存先を選択")
        .set_file_name(suggested_name.unwrap_or_else(|| "video.mp4".to_string()))
        .add_filter("MP4動画", &["mp4"])
        .save_file()
        .map(|path| {
            if path.extension().is_none() {
                path.with_extension("mp4").to_string_lossy().into_owned()
            } else {
                path.to_string_lossy().into_owned()
            }
        })
}
