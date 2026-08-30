use crate::portable::PortablePaths;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::{Command, Stdio},
};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TechnicalMetadata {
    pub frame_rate: String,
    pub resolution: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub color_space: String,
    pub color_primaries: String,
    pub transfer_characteristics: String,
    pub rotation_orientation: String,
    pub timecode: String,
    pub encoder_version: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableMetadata {
    pub title: String,
    pub artist_author: String,
    pub creator: String,
    pub comment: String,
    pub description: String,
    pub copyright: String,
    pub creation_time: String,
    pub modification_time: String,
    pub encoder: String,
    pub encoded_by: String,
    pub software: String,
    pub version: String,
    pub publisher: String,
    pub genre: String,
    pub language: String,
    pub location: String,
    pub keywords: String,
    pub project_name: String,
    pub project_id: String,
    pub asset_id: String,
    pub uuid: String,
    pub source: String,
    pub edit_software: String,
    pub export_preset: String,
    pub encoder_version: String,
    pub handler_name: String,
    pub gps: String,
    pub camera_device: String,
    pub custom_metadata: String,
    pub xmp: String,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub technical: TechnicalMetadata,
    pub editable: EditableMetadata,
}

pub fn inspect(
    paths: &PortablePaths,
    input: &Path,
    details: &str,
    width: u32,
    height: u32,
) -> VideoMetadata {
    let tags = read_global_tags(paths, input);
    let video_line = stream_line(details, "Video:").unwrap_or_default();
    let audio_line = stream_line(details, "Audio:").unwrap_or_default();
    let (color_space, color_primaries, transfer_characteristics) = color_information(video_line);
    let rotation = parse_rotation(details).unwrap_or(0.0);
    let orientation = if height > width {
        "縦向き"
    } else if width > height {
        "横向き"
    } else {
        "正方形"
    };
    let timecode = tag(&tags, &["timecode"])
        .or_else(|| parse_detail_tag(details, "timecode"))
        .unwrap_or_default();
    let encoder_versions = parse_encoder_versions(details);

    let editable = editable_from_tags(&tags, details, &encoder_versions);
    VideoMetadata {
        technical: TechnicalMetadata {
            frame_rate: parse_frame_rate(video_line).unwrap_or_default(),
            resolution: format!("{width} × {height}"),
            video_codec: parse_codec(video_line, "Video:").unwrap_or_default(),
            audio_codec: parse_codec(audio_line, "Audio:").unwrap_or_else(|| "なし".to_string()),
            color_space,
            color_primaries,
            transfer_characteristics,
            rotation_orientation: format!("{rotation:.0}° / {orientation}"),
            timecode,
            encoder_version: encoder_versions,
        },
        editable,
    }
}

pub fn apply_to_command(command: &mut Command, metadata: &EditableMetadata) -> Result<(), String> {
    validate(metadata)?;
    for (key, value) in [
        ("title", &metadata.title),
        ("artist", &metadata.artist_author),
        ("creator", &metadata.creator),
        ("comment", &metadata.comment),
        ("description", &metadata.description),
        ("copyright", &metadata.copyright),
        ("creation_time", &metadata.creation_time),
        ("modification_time", &metadata.modification_time),
        ("encoder", &metadata.encoder),
        ("encoded_by", &metadata.encoded_by),
        ("software", &metadata.software),
        ("version", &metadata.version),
        ("publisher", &metadata.publisher),
        ("genre", &metadata.genre),
        ("language", &metadata.language),
        ("location_name", &metadata.location),
        ("keywords", &metadata.keywords),
        ("project_name", &metadata.project_name),
        ("project_id", &metadata.project_id),
        ("asset_id", &metadata.asset_id),
        ("uuid", &metadata.uuid),
        ("source", &metadata.source),
        ("edit_software", &metadata.edit_software),
        ("export_preset", &metadata.export_preset),
        ("encoder_version", &metadata.encoder_version),
        ("location", &metadata.gps),
        ("camera_device", &metadata.camera_device),
        ("xmp", &metadata.xmp),
    ] {
        add_metadata(command, key, value);
    }
    if !metadata.handler_name.trim().is_empty() {
        command
            .arg("-metadata:s:v:0")
            .arg(format!("handler_name={}", metadata.handler_name.trim()));
    }
    for (key, value) in parse_custom_metadata(&metadata.custom_metadata)? {
        add_metadata(command, &key, &value);
    }
    Ok(())
}

fn add_metadata(command: &mut Command, key: &str, value: &str) {
    if !value.trim().is_empty() {
        command
            .arg("-metadata")
            .arg(format!("{key}={}", value.trim()));
    }
}

fn validate(metadata: &EditableMetadata) -> Result<(), String> {
    for value in [
        &metadata.title,
        &metadata.artist_author,
        &metadata.creator,
        &metadata.comment,
        &metadata.description,
        &metadata.copyright,
        &metadata.creation_time,
        &metadata.modification_time,
        &metadata.encoder,
        &metadata.encoded_by,
        &metadata.software,
        &metadata.version,
        &metadata.publisher,
        &metadata.genre,
        &metadata.language,
        &metadata.location,
        &metadata.keywords,
        &metadata.project_name,
        &metadata.project_id,
        &metadata.asset_id,
        &metadata.uuid,
        &metadata.source,
        &metadata.edit_software,
        &metadata.export_preset,
        &metadata.encoder_version,
        &metadata.handler_name,
        &metadata.gps,
        &metadata.camera_device,
    ] {
        if value.chars().count() > 4096 {
            return Err("メタデータの各項目は4096文字以内で入力してください。".to_string());
        }
    }
    if metadata.xmp.chars().count() > 65_535 || metadata.custom_metadata.chars().count() > 65_535 {
        return Err("XMPと独自メタデータは65535文字以内で入力してください。".to_string());
    }
    parse_custom_metadata(&metadata.custom_metadata)?;
    Ok(())
}

fn parse_custom_metadata(value: &str) -> Result<Vec<(String, String)>, String> {
    let key_pattern =
        Regex::new(r"^[A-Za-z0-9_.:-]{1,64}$").expect("valid custom metadata key regex");
    let mut parsed = Vec::new();
    for (index, line) in value.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            format!(
                "独自メタデータの{}行目は key=value 形式で入力してください。",
                index + 1
            )
        })?;
        let key = key.trim();
        if !key_pattern.is_match(key) {
            return Err(format!("独自メタデータのキーが不正です: {key}"));
        }
        if key.to_ascii_lowercase().contains("c2pa") {
            return Err("C2PAは確認専用のため、独自メタデータとして変更できません。".to_string());
        }
        parsed.push((key.to_string(), value.trim().to_string()));
    }
    Ok(parsed)
}

fn read_global_tags(paths: &PortablePaths, input: &Path) -> BTreeMap<String, String> {
    let ffmpeg = match paths.ffmpeg_path() {
        Ok(path) => path,
        Err(_) => return BTreeMap::new(),
    };
    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-i")
        .arg(input)
        .arg("-f")
        .arg("ffmetadata")
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    hide_console_window(&mut command);
    let output = match command.output() {
        Ok(output) if output.status.success() => output,
        _ => return BTreeMap::new(),
    };
    parse_ffmetadata(&String::from_utf8_lossy(&output.stdout))
}

fn parse_ffmetadata(value: &str) -> BTreeMap<String, String> {
    value
        .lines()
        .filter(|line| !line.starts_with(';') && !line.trim().is_empty())
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            (
                key.trim().to_ascii_lowercase(),
                unescape_ffmetadata(value.trim()),
            )
        })
        .collect()
}

fn unescape_ffmetadata(value: &str) -> String {
    let mut result = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            result.push(match character {
                'n' => '\n',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            result.push(character);
        }
    }
    if escaped {
        result.push('\\');
    }
    result
}

fn editable_from_tags(
    tags: &BTreeMap<String, String>,
    details: &str,
    encoder_versions: &str,
) -> EditableMetadata {
    let known = known_keys();
    let custom_metadata = tags
        .iter()
        .filter(|(key, _)| {
            !known.contains(key.as_str()) && !key.to_ascii_lowercase().contains("c2pa")
        })
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    EditableMetadata {
        title: tag(tags, &["title"]).unwrap_or_default(),
        artist_author: tag(tags, &["artist", "author"]).unwrap_or_default(),
        creator: tag(tags, &["creator"]).unwrap_or_default(),
        comment: tag(tags, &["comment"]).unwrap_or_default(),
        description: tag(tags, &["description", "synopsis"]).unwrap_or_default(),
        copyright: tag(tags, &["copyright"]).unwrap_or_default(),
        creation_time: tag(tags, &["creation_time"]).unwrap_or_default(),
        modification_time: tag(tags, &["modification_time"]).unwrap_or_default(),
        encoder: tag(tags, &["encoder"]).unwrap_or_default(),
        encoded_by: tag(tags, &["encoded_by"]).unwrap_or_default(),
        software: tag(tags, &["software"]).unwrap_or_default(),
        version: tag(tags, &["version"]).unwrap_or_default(),
        publisher: tag(tags, &["publisher"]).unwrap_or_default(),
        genre: tag(tags, &["genre"]).unwrap_or_default(),
        language: tag(tags, &["language"]).unwrap_or_default(),
        location: tag(tags, &["location_name"]).unwrap_or_default(),
        keywords: tag(tags, &["keywords"]).unwrap_or_default(),
        project_name: tag(tags, &["project_name"]).unwrap_or_default(),
        project_id: tag(tags, &["project_id"]).unwrap_or_default(),
        asset_id: tag(tags, &["asset_id"]).unwrap_or_default(),
        uuid: tag(tags, &["uuid"]).unwrap_or_default(),
        source: tag(tags, &["source"]).unwrap_or_default(),
        edit_software: tag(tags, &["edit_software"]).unwrap_or_default(),
        export_preset: tag(tags, &["export_preset"]).unwrap_or_default(),
        encoder_version: tag(tags, &["encoder_version"])
            .unwrap_or_else(|| encoder_versions.to_string()),
        handler_name: parse_detail_tag(details, "handler_name").unwrap_or_default(),
        gps: tag(tags, &["location"]).unwrap_or_default(),
        camera_device: tag(tags, &["camera_device", "camera", "device"]).unwrap_or_default(),
        custom_metadata,
        xmp: tag(tags, &["xmp"]).unwrap_or_default(),
    }
}

fn known_keys() -> BTreeSet<&'static str> {
    [
        "major_brand",
        "minor_version",
        "compatible_brands",
        "title",
        "artist",
        "author",
        "creator",
        "comment",
        "description",
        "synopsis",
        "copyright",
        "creation_time",
        "modification_time",
        "encoder",
        "encoded_by",
        "software",
        "version",
        "publisher",
        "genre",
        "language",
        "location_name",
        "keywords",
        "project_name",
        "project_id",
        "asset_id",
        "uuid",
        "source",
        "edit_software",
        "export_preset",
        "encoder_version",
        "location",
        "camera_device",
        "camera",
        "device",
        "xmp",
        "timecode",
    ]
    .into_iter()
    .collect()
}

fn tag(tags: &BTreeMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| tags.get(*key).cloned())
        .filter(|value| !value.is_empty())
}

fn stream_line<'a>(details: &'a str, kind: &str) -> Option<&'a str> {
    details
        .lines()
        .find(|line| line.contains("Stream #") && line.contains(kind))
}

fn parse_codec(line: &str, kind: &str) -> Option<String> {
    let value = line.split_once(kind)?.1.trim();
    let codec = value
        .split(|character: char| character == ' ' || character == ',' || character == '(')
        .next()?;
    Some(match codec.to_ascii_lowercase().as_str() {
        "h264" => "H.264".to_string(),
        "hevc" | "h265" => "H.265 / HEVC".to_string(),
        "aac" => "AAC".to_string(),
        "opus" => "Opus".to_string(),
        "vp9" => "VP9".to_string(),
        "av1" => "AV1".to_string(),
        other => other.to_ascii_uppercase(),
    })
}

fn parse_frame_rate(line: &str) -> Option<String> {
    let pattern = Regex::new(r"(?:,|\s)(\d+(?:\.\d+)?) fps(?:,|\s|$)").ok()?;
    let value = pattern.captures(line)?.get(1)?.as_str();
    Some(format!("{value} fps"))
}

fn color_information(line: &str) -> (String, String, String) {
    let pattern = Regex::new(r"\b((?:yuv|yuva|gbr|rgb)[A-Za-z0-9_]*)(?:\(([^)]*)\))?")
        .expect("valid color regex");
    let Some(captures) = pattern.captures(line) else {
        return (String::new(), String::new(), String::new());
    };
    let pixel_format = captures
        .get(1)
        .map(|value| value.as_str())
        .unwrap_or_default();
    let details = captures
        .get(2)
        .map(|value| value.as_str())
        .unwrap_or_default();
    let slash_values = details
        .split(',')
        .map(str::trim)
        .find(|value| value.contains('/'))
        .map(|value| value.split('/').map(str::trim).collect::<Vec<_>>())
        .unwrap_or_default();
    let color_space = if let Some(value) = slash_values.first() {
        format!("{pixel_format} / {value}")
    } else {
        pixel_format.to_string()
    };
    let primaries = slash_values.get(1).copied().unwrap_or_default().to_string();
    let transfer = slash_values.get(2).copied().unwrap_or_default();
    let transfer = if transfer.eq_ignore_ascii_case("smpte2084") {
        format!("{transfer}（HDR / PQ）")
    } else if transfer.eq_ignore_ascii_case("arib-std-b67") {
        format!("{transfer}（HDR / HLG）")
    } else if !transfer.is_empty() {
        format!("{transfer}（SDR）")
    } else {
        String::new()
    };
    (color_space, primaries, transfer)
}

fn parse_rotation(details: &str) -> Option<f64> {
    Regex::new(r"rotation of\s+(-?\d+(?:\.\d+)?)")
        .ok()?
        .captures(details)?
        .get(1)?
        .as_str()
        .parse::<f64>()
        .ok()
}

fn parse_detail_tag(details: &str, key: &str) -> Option<String> {
    let pattern = Regex::new(&format!(r"(?im)^\s*{}\s*:\s*(.+?)\s*$", regex::escape(key))).ok()?;
    pattern
        .captures(details)?
        .get(1)
        .map(|value| value.as_str().trim().to_string())
}

fn parse_encoder_versions(details: &str) -> String {
    let pattern = Regex::new(r"(?im)^\s*encoder\s*:\s*(.+?)\s*$").expect("valid encoder regex");
    let mut values = BTreeSet::new();
    for captures in pattern.captures_iter(details) {
        if let Some(value) = captures.get(1) {
            values.insert(value.as_str().trim().to_string());
        }
    }
    values.into_iter().collect::<Vec<_>>().join(" / ")
}

fn hide_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        color_information, editable_from_tags, parse_custom_metadata, parse_ffmetadata,
        parse_frame_rate, parse_rotation,
    };

    #[test]
    fn parses_technical_metadata() {
        let line = "Stream #0:0: Video: hevc, yuv420p10le(tv, bt2020nc/bt2020/smpte2084, progressive), 1920x1080, 29.97 fps";
        assert_eq!(parse_frame_rate(line).as_deref(), Some("29.97 fps"));
        let color = color_information(line);
        assert!(color.0.contains("bt2020nc"));
        assert_eq!(color.1, "bt2020");
        assert!(color.2.contains("HDR / PQ"));
        assert_eq!(parse_rotation("rotation of -90.00 degrees"), Some(-90.0));
    }

    #[test]
    fn parses_ffmetadata_values() {
        let tags = parse_ffmetadata(";FFMETADATA1\ntitle=Hello\\=World\ncomment=Line\\nTwo\n");
        assert_eq!(tags.get("title").map(String::as_str), Some("Hello=World"));
        assert_eq!(tags.get("comment").map(String::as_str), Some("Line\nTwo"));
    }

    #[test]
    fn keeps_c2pa_read_only() {
        let tags = parse_ffmetadata(";FFMETADATA1\nc2pa.manifest=provenance\ncustom_key=value\n");
        let editable = editable_from_tags(&tags, "", "");
        assert!(!editable.custom_metadata.contains("c2pa"));
        assert!(editable.custom_metadata.contains("custom_key=value"));
        assert!(parse_custom_metadata("C2PA=value").is_err());
    }
}
