use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::http::{Request, Response};

const MAX_CHUNK_SIZE: u64 = 4 * 1024 * 1024;

pub type PreviewStore = Arc<Mutex<Option<PathBuf>>>;

pub fn response(request: &Request<Vec<u8>>, preview: &PreviewStore) -> Response<Vec<u8>> {
    let path = match preview.lock().ok().and_then(|value| value.clone()) {
        Some(path) if path.is_file() => path,
        _ => return error_response(404, "Preview video is not available"),
    };

    match read_video_range(request, &path) {
        Ok(response) => response,
        Err(message) => error_response(500, &message),
    }
}

fn read_video_range(
    request: &Request<Vec<u8>>,
    path: &PathBuf,
) -> Result<Response<Vec<u8>>, String> {
    let mut file = File::open(path).map_err(|error| format!("Could not open preview: {error}"))?;
    let file_size = file
        .metadata()
        .map_err(|error| format!("Could not read preview metadata: {error}"))?
        .len();
    if file_size == 0 {
        return Err("Preview file is empty".to_string());
    }

    let requested = request
        .headers()
        .get("range")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_range(value, file_size));
    let (start, requested_end) = requested.unwrap_or((0, file_size - 1));
    let end = requested_end.min(start.saturating_add(MAX_CHUNK_SIZE - 1));
    if start >= file_size || end < start {
        return Ok(Response::builder()
            .status(416)
            .header("Content-Range", format!("bytes */{file_size}"))
            .body(Vec::new())
            .expect("valid range response"));
    }

    let length = end - start + 1;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("Could not seek preview: {error}"))?;
    let mut body = vec![0_u8; length as usize];
    file.read_exact(&mut body)
        .map_err(|error| format!("Could not read preview: {error}"))?;

    let partial = requested.is_some() || start > 0 || end < file_size - 1;
    let mut builder = Response::builder()
        .status(if partial { 206 } else { 200 })
        .header("Content-Type", content_type(path))
        .header("Accept-Ranges", "bytes")
        .header("Content-Length", length.to_string())
        .header("Cache-Control", "no-store");
    if partial {
        builder = builder.header("Content-Range", format!("bytes {start}-{end}/{file_size}"));
    }
    builder
        .body(body)
        .map_err(|error| format!("Could not build preview response: {error}"))
}

fn content_type(path: &PathBuf) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("mkv") => "video/x-matroska",
        Some("wmv") => "video/x-ms-wmv",
        _ => "video/mp4",
    }
}

fn parse_range(value: &str, file_size: u64) -> Option<(u64, u64)> {
    let value = value.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?.min(file_size);
        return Some((file_size.saturating_sub(suffix), file_size - 1));
    }
    let start = start.parse::<u64>().ok()?;
    let end = if end.is_empty() {
        file_size - 1
    } else {
        end.parse::<u64>().ok()?.min(file_size - 1)
    };
    Some((start, end))
}

fn error_response(status: u16, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(message.as_bytes().to_vec())
        .expect("valid error response")
}

#[cfg(test)]
mod tests {
    use super::{parse_range, response, PreviewStore};
    use std::{
        fs,
        sync::{Arc, Mutex},
    };
    use tauri::http::Request;

    #[test]
    fn parses_open_and_suffix_ranges() {
        assert_eq!(parse_range("bytes=10-", 100), Some((10, 99)));
        assert_eq!(parse_range("bytes=-20", 100), Some((80, 99)));
        assert_eq!(parse_range("bytes=5-14", 100), Some((5, 14)));
    }

    #[test]
    fn serves_only_the_requested_preview_range() {
        let path =
            std::env::temp_dir().join(format!("qvm-preview-range-{}.mp4", std::process::id()));
        let bytes = (0_u8..=255).collect::<Vec<_>>();
        fs::write(&path, &bytes).expect("preview fixture");
        let store: PreviewStore = Arc::new(Mutex::new(Some(path.clone())));
        let request = Request::builder()
            .uri("qvm://localhost/preview")
            .header("Range", "bytes=20-39")
            .body(Vec::new())
            .expect("range request");
        let response = response(&request, &store);
        assert_eq!(response.status(), 206);
        assert_eq!(response.body(), &bytes[20..40]);
        let _ = fs::remove_file(path);
    }
}
