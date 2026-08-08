use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const FFMPEG_BYTES: &[u8] = include_bytes!("../../node_modules/ffmpeg-static/ffmpeg.exe");

#[derive(Clone)]
pub struct PortablePaths {
    pub root: PathBuf,
    pub settings: PathBuf,
    pub logs: PathBuf,
    pub cache: PathBuf,
    pub temp: PathBuf,
    pub webview: PathBuf,
}

impl PortablePaths {
    pub fn initialize() -> Result<Self, String> {
        let executable = env::current_exe()
            .map_err(|error| format!("実行ファイルの場所を取得できません: {error}"))?;
        let executable_dir = executable
            .parent()
            .ok_or_else(|| "実行ファイルの親フォルダーを取得できません".to_string())?;
        let root = executable_dir.join("QuickVideoMaker-PortableData");
        let paths = Self {
            settings: root.join("settings"),
            logs: root.join("logs"),
            cache: root.join("cache"),
            temp: root.join("temp"),
            webview: root.join("WebView"),
            root,
        };

        for directory in [
            &paths.root,
            &paths.settings,
            &paths.logs,
            &paths.cache,
            &paths.temp,
            &paths.webview,
        ] {
            fs::create_dir_all(directory).map_err(|error| {
                format!(
                    "ポータブルデータ用フォルダーを作成できません（{}）: {error}",
                    directory.display()
                )
            })?;
        }

        paths.clean_temp()?;
        paths.clean_preview_cache()?;
        paths.ensure_settings_file()?;
        paths.configure_environment();
        let ffmpeg = paths.ffmpeg_path()?;
        paths.log("application startup");
        paths.log(&format!("bundled ffmpeg ready: {}", ffmpeg.display()));
        Ok(paths)
    }

    fn configure_environment(&self) {
        env::set_var("WEBVIEW2_USER_DATA_FOLDER", &self.webview);
        env::set_var("TEMP", &self.temp);
        env::set_var("TMP", &self.temp);
    }

    fn ensure_settings_file(&self) -> Result<(), String> {
        let settings_file = self.settings.join("app.json");
        if !settings_file.exists() {
            fs::write(&settings_file, "{\n  \"dataFormatVersion\": 1\n}\n")
                .map_err(|error| format!("設定ファイルを作成できません: {error}"))?;
        }
        Ok(())
    }

    pub fn ffmpeg_path(&self) -> Result<PathBuf, String> {
        let tool_dir = self.cache.join("tools");
        fs::create_dir_all(&tool_dir)
            .map_err(|error| format!("FFmpeg用フォルダーを作成できません: {error}"))?;
        let destination = tool_dir.join("ffmpeg.exe");

        let embedded_hash = Sha256::digest(FFMPEG_BYTES);
        let installed_matches = hash_file(&destination)
            .map(|hash| hash.as_slice() == embedded_hash.as_slice())
            .unwrap_or(false);

        if !installed_matches {
            let temporary = tool_dir.join("ffmpeg.exe.new");
            fs::write(&temporary, FFMPEG_BYTES)
                .map_err(|error| format!("同梱FFmpegを展開できません: {error}"))?;
            if destination.exists() {
                fs::remove_file(&destination)
                    .map_err(|error| format!("古いFFmpegを更新できません: {error}"))?;
            }
            fs::rename(&temporary, &destination)
                .map_err(|error| format!("FFmpegを配置できません: {error}"))?;
        }

        Ok(destination)
    }

    pub fn session_temp(&self) -> Result<PathBuf, String> {
        let session = self.temp.join(format!("session-{}", std::process::id()));
        fs::create_dir_all(&session)
            .map_err(|error| format!("一時フォルダーを作成できません: {error}"))?;
        Ok(session)
    }

    pub fn clean_temp(&self) -> Result<(), String> {
        if self.temp.exists() {
            for entry in fs::read_dir(&self.temp)
                .map_err(|error| format!("一時フォルダーを読み取れません: {error}"))?
            {
                let path = entry
                    .map_err(|error| format!("一時ファイルを読み取れません: {error}"))?
                    .path();
                if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                }
                .map_err(|error| {
                    format!("一時データを削除できません（{}）: {error}", path.display())
                })?;
            }
        }
        fs::create_dir_all(&self.temp)
            .map_err(|error| format!("一時フォルダーを再作成できません: {error}"))
    }

    pub fn clean_preview_cache(&self) -> Result<(), String> {
        let preview_cache = self.cache.join("previews");
        if preview_cache.exists() {
            fs::remove_dir_all(&preview_cache).map_err(|error| {
                format!(
                    "プレビューキャッシュを削除できません（{}）: {error}",
                    preview_cache.display()
                )
            })?;
        }
        fs::create_dir_all(&preview_cache)
            .map_err(|error| format!("プレビューキャッシュを作成できません: {error}"))
    }

    pub fn log(&self, message: &str) {
        let log_path = self.logs.join("QuickVideoMaker.log");
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_secs())
                .unwrap_or_default();
            let _ = writeln!(file, "[{timestamp}] {message}");
        }
    }
}

fn hash_file(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_vec())
}
