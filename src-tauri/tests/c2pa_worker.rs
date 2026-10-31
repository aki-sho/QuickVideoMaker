use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[test]
fn c2pa_worker_reads_unsigned_video_without_starting_the_app() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/security-tests");
    std::fs::create_dir_all(&parent).unwrap();
    let directory = tempfile::tempdir_in(parent).unwrap();
    let input = directory.path().join("unsigned.mp4");
    let ffmpeg = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.tools/ffmpeg/ffmpeg.exe");
    let status = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=32x32:d=0.1",
            "-c:v",
            "libx264",
        ])
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());
    let mut worker = Command::new(env!("CARGO_BIN_EXE_quick-video-maker"))
        .arg("--qvm-c2pa-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    worker
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input.to_string_lossy()).unwrap())
        .unwrap();
    let started = std::time::Instant::now();
    while worker.try_wait().unwrap().is_none() {
        if started.elapsed() > std::time::Duration::from_secs(30) {
            let _ = worker.kill();
            let _ = worker.wait();
            panic!("C2PA worker did not finish within its deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = worker.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let details: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(details["validationResult"], "証明情報なし");
}
