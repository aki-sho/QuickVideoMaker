use crate::media_security::ensure_local_path;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

pub fn validate(value: &str, protected: &[&Path]) -> Result<PathBuf, String> {
    if value.trim().is_empty() {
        return Err("保存先を選択してください。".to_string());
    }
    let mut path = PathBuf::from(value);
    ensure_local_path(&path)?;
    if path.file_name().is_none() {
        return Err("保存するファイル名を指定してください。".to_string());
    }
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
    {
        path.set_extension("mp4");
    }
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|e| format!("保存先を解決できません: {e}"))?
            .join(path)
    };
    check_protected(&absolute, protected)?;
    Ok(absolute)
}

fn check_protected(output: &Path, protected: &[&Path]) -> Result<(), String> {
    match fs::symlink_metadata(output) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("保存先には通常のファイル名を指定してください。".to_string());
            }
            for input in protected {
                // File identity also detects hard links, different case and path aliases.
                if same_file::is_same_file(output, input)
                    .map_err(|e| format!("保存先の同一性を確認できません: {e}"))?
                {
                    return Err(
                        "入力素材と同じファイルには保存できません。別の名前で保存してください。"
                            .to_string(),
                    );
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("保存先を確認できません: {error}")),
    }
    Ok(())
}

pub fn save(source: &Path, output: &Path, protected: &[&Path]) -> Result<(), String> {
    let file = File::open(source).map_err(|e| format!("完成動画を読み取れません: {e}"))?;
    if file.metadata().map_err(|e| e.to_string())?.len() == 0 {
        return Err("完成動画が空です。".to_string());
    }
    save_reader(file, output, protected)
}

fn save_reader(mut source: impl Read, output: &Path, protected: &[&Path]) -> Result<(), String> {
    ensure_local_path(output)?;
    check_protected(output, protected)?;
    let parent = output
        .parent()
        .ok_or_else(|| "保存先フォルダーがありません。".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("保存先フォルダーを作成できません: {e}"))?;
    let parent = fs::canonicalize(parent).map_err(|e| format!("保存先を解決できません: {e}"))?;
    ensure_local_path(&parent)?;
    let destination = parent.join(
        output
            .file_name()
            .ok_or("保存するファイル名がありません。")?,
    );
    // Stage on the destination volume. An incomplete copy never touches the old file.
    let mut staged = tempfile::Builder::new()
        .prefix(".qvm-save-")
        .suffix(".tmp")
        .tempfile_in(&parent)
        .map_err(|e| format!("保存用の一時ファイルを作成できません: {e}"))?;
    io::copy(&mut source, staged.as_file_mut())
        .map_err(|e| format!("動画を保存できません（既存ファイルは保持されています）: {e}"))?;
    staged
        .as_file()
        .sync_all()
        .map_err(|e| format!("動画をディスクへ書き込めません: {e}"))?;
    check_protected(&destination, protected)?;
    staged.persist(&destination).map_err(|e| {
        format!(
            "保存ファイルを置き換えられません（既存ファイルは保持されています）: {}",
            e.error
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_paths_and_input_aliases_including_hard_links() {
        let dir = crate::security_test_dir();
        let input = dir.path().join("source.mp4");
        let alias = dir.path().join("alias.mp4");
        fs::write(&input, b"original").unwrap();
        fs::hard_link(&input, &alias).unwrap();
        assert!(validate("", &[&input]).is_err());
        assert!(validate(
            &dir.path().join(".\\source.mp4").to_string_lossy(),
            &[&input]
        )
        .is_err());
        assert!(validate(&alias.to_string_lossy(), &[&input]).is_err());
        assert_eq!(fs::read(input).unwrap(), b"original");
    }

    #[test]
    fn failed_copy_preserves_existing_output_and_removes_staging_file() {
        struct BrokenReader;
        impl Read for BrokenReader {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("simulated disk failure"))
            }
        }
        let dir = crate::security_test_dir();
        let output = dir.path().join("output.mp4");
        fs::write(&output, b"old video").unwrap();
        assert!(save_reader(BrokenReader, &output, &[]).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"old video");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        save_reader(&b"new video"[..], &output, &[]).unwrap();
        assert_eq!(fs::read(output).unwrap(), b"new video");
    }
}
