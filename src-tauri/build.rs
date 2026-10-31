fn main() {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-changed=../scripts/ffmpeg-release.json");
    println!("cargo:rerun-if-changed=../.tools/ffmpeg/ffmpeg.exe");
    let release: serde_json::Value =
        serde_json::from_str(include_str!("../scripts/ffmpeg-release.json"))
            .expect("FFmpeg release manifest");
    let mut file = std::fs::File::open("../.tools/ffmpeg/ffmpeg.exe")
        .expect("Run node scripts/prepare-ffmpeg.mjs before building");
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).expect("Read bundled FFmpeg");
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    assert_eq!(
        format!("{:x}", hasher.finalize()),
        release["binarySha256"].as_str().unwrap(),
        "Bundled FFmpeg hash mismatch; run node scripts/prepare-ffmpeg.mjs"
    );
    tauri_build::build()
}
