use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy)]
pub enum InputKind {
    Audio,
    Image,
    Video,
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn validate_input(value: &str, kind: InputKind, label: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    ensure_local_path(&path)?;
    let path =
        fs::canonicalize(path).map_err(|e| format!("{label}ファイルを読み取れません: {e}"))?;
    ensure_local_path(&path)?;
    let info = fs::metadata(&path).map_err(|e| format!("{label}ファイルを読み取れません: {e}"))?;
    if !info.is_file() || info.len() == 0 || info.len() > 100 * 1024 * 1024 * 1024 {
        return Err(format!(
            "{label}には、空でない100GB以下の通常ファイルを指定してください。"
        ));
    }
    let ext = extension(&path);
    let allowed = match kind {
        InputKind::Audio => matches!(
            ext.as_str(),
            "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "opus" | "wma"
        ),
        InputKind::Image => matches!(
            ext.as_str(),
            "jpg" | "jpeg" | "png" | "bmp" | "webp" | "gif" | "tif" | "tiff"
        ),
        InputKind::Video => matches!(
            ext.as_str(),
            "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "wmv"
        ),
    };
    if !allowed {
        return Err(format!("対応していない{label}形式です。"));
    }
    Ok(path)
}

/// Restrict the demuxer as well as protocols: renaming a playlist or script to
/// .mp4/.png cannot enable HLS, concat, AviSynth, or an image sequence.
pub fn add_input(command: &mut Command, path: &Path, still_loop: bool) -> Result<(), String> {
    let ext = extension(path);
    let (format, codec) = match ext.as_str() {
        "mp4" | "mov" | "m4v" | "m4a" => ("mov", None),
        "avi" => ("avi", None),
        "mkv" | "webm" => ("matroska", None),
        "wmv" | "wma" => ("asf", None),
        "mp3" => ("mp3", None),
        "wav" => ("wav", None),
        "aac" => ("aac", None),
        "flac" => ("flac", None),
        "ogg" | "opus" => ("ogg", None),
        "jpg" | "jpeg" => ("image2", Some("mjpeg")),
        "png" => ("image2", Some("png")),
        "bmp" => ("image2", Some("bmp")),
        "webp" => ("image2", Some("webp")),
        "tif" | "tiff" => ("image2", Some("tiff")),
        "gif" => ("gif", None),
        _ => return Err("対応していない入力形式です。".to_string()),
    };
    command.args([
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        format,
        "-f",
        format,
        "-probesize",
        "8388608",
        "-analyzeduration",
        "10000000",
        "-max_streams",
        "64",
        "-threads",
        "2",
        "-max_pixels",
        "67108864",
    ]);
    if format == "mov" {
        command.args(["-enable_drefs", "0", "-use_absolute_path", "0"]);
    }
    if let Some(codec) = codec {
        command.args(["-pattern_type", "none", "-c:v", codec]);
        if still_loop {
            command.args(["-loop", "1", "-framerate", "30"]);
        }
    } else if still_loop {
        command.args(["-stream_loop", "-1"]);
    }
    command.arg("-i").arg(path);
    Ok(())
}

pub fn ffmpeg_command(executable: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(executable);
    command.args([
        "-hide_banner",
        "-nostdin",
        "-max_alloc",
        "134217728",
        "-filter_threads",
        "2",
        "-filter_complex_threads",
        "2",
    ]);
    command
}

pub fn ensure_local_path(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        let drive = match absolute.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
                _ => return Err("ネットワークパス・デバイスパスは使用できません。ローカルファイルを選択してください。".to_string()),
            },
            _ => return Err("ローカルドライブ上のファイルを指定してください。".to_string()),
        };
        let drive_root = [drive as u16, b':' as u16, b'\\' as u16, 0];
        let drive_type =
            unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(drive_root.as_ptr()) };
        if !matches!(drive_type, 2 | 3 | 5 | 6) {
            return Err("ローカルドライブ上のファイルを指定してください。".to_string());
        }
        for component in absolute.components() {
            if let Component::Normal(name) = component {
                let name = name.to_string_lossy();
                let stem = name
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                if name.contains(':')
                    || name.ends_with([' ', '.'])
                    || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    || (stem.len() == 4
                        && (stem.starts_with("COM") || stem.starts_with("LPT"))
                        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
                {
                    return Err(
                        "特殊なデバイス名・代替データストリームは使用できません。".to_string()
                    );
                }
            }
        }
    }
    let _ = (path, std::mem::size_of::<Component<'_>>());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicitly_disables_network_and_sequence_or_script_demuxers() {
        let mut command = ffmpeg_command("ffmpeg");
        add_input(&mut command, Path::new("untrusted.png"), true).unwrap();
        let args = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args
            .windows(2)
            .any(|a| a == ["-protocol_whitelist", "file"]));
        assert!(args
            .windows(2)
            .any(|a| a == ["-format_whitelist", "image2"]));
        assert!(args.windows(2).any(|a| a == ["-pattern_type", "none"]));
        assert!(add_input(&mut command, Path::new("input.avs"), false).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_unc_devices_and_alternate_streams() {
        for path in [
            r"\\server\share\video.mp4",
            r"\\.\PhysicalDrive0",
            r"C:\video.mp4:secret",
            r"C:\NUL.mp4",
        ] {
            assert!(ensure_local_path(Path::new(path)).is_err(), "{path}");
        }
    }

    #[test]
    fn rejects_a_renamed_network_playlist_without_contacting_the_server() {
        use std::{io, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let directory = crate::security_test_dir();
        let input = directory.path().join("disguised.mp4");
        fs::write(&input, format!("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nhttp://{}/secret.ts\n#EXT-X-ENDLIST\n", listener.local_addr().unwrap())).unwrap();
        let path = validate_input(&input.to_string_lossy(), InputKind::Video, "動画").unwrap();
        let mut command = ffmpeg_command(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.tools/ffmpeg/ffmpeg.exe"),
        );
        add_input(&mut command, &path, false).unwrap();
        let output = crate::subprocess::capture(
            command,
            crate::subprocess::ANALYSIS_TIMEOUT,
            crate::subprocess::MAX_CAPTURE,
            None,
        )
        .unwrap();
        assert!(!output.status.success());
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn decodes_supported_image_formats_with_restricted_inputs() {
        let directory = crate::security_test_dir();
        let ffmpeg = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.tools/ffmpeg/ffmpeg.exe");
        for extension in ["png", "jpg", "bmp", "webp", "gif", "tif"] {
            let input = directory.path().join(format!("image.{extension}"));
            let mut create = Command::new(&ffmpeg);
            create
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=blue:s=32x32",
                    "-frames:v",
                    "1",
                ])
                .arg(&input);
            let created = crate::subprocess::capture(
                create,
                crate::subprocess::ANALYSIS_TIMEOUT,
                crate::subprocess::MAX_CAPTURE,
                None,
            )
            .unwrap();
            assert!(
                created.status.success(),
                "{extension}: {}",
                String::from_utf8_lossy(&created.stderr)
            );
            let mut command = ffmpeg_command(&ffmpeg);
            add_input(&mut command, &input, false).unwrap();
            command.args(["-frames:v", "1", "-f", "null", "-"]);
            let decoded = crate::subprocess::capture(
                command,
                crate::subprocess::ANALYSIS_TIMEOUT,
                crate::subprocess::MAX_CAPTURE,
                None,
            )
            .unwrap();
            assert!(
                decoded.status.success(),
                "{extension}: {}",
                String::from_utf8_lossy(&decoded.stderr)
            );
        }
    }
}
