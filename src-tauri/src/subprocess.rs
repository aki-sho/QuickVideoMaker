//! Bounded child-process I/O. Untrusted media never gets an unlimited capture or wait.
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub const ANALYSIS_TIMEOUT: Duration = Duration::from_secs(30);
pub const ANALYSIS_MEMORY: usize = 512 * 1024 * 1024;
pub const MAX_CAPTURE: usize = 2 * 1024 * 1024;
// A permitted 65,535-character XMP field can use four UTF-8 bytes per character.
const MAX_LINE: usize = 512 * 1024;
const MAX_ENCODE_OUTPUT: usize = 64 * 1024 * 1024;
const ENCODE_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

pub fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = command;
}

#[cfg(windows)]
pub struct ProcessJob {
    _handle: std::os::windows::io::OwnedHandle,
}

#[cfg(windows)]
impl ProcessJob {
    pub fn attach(child: &Child, memory: usize) -> Result<Self, String> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::*;
        // The owned job handle closes on every return path, killing its process tree.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(format!(
                "解析プロセスの制限を作成できません: {}",
                std::io::Error::last_os_error()
            ));
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_JOB_MEMORY;
        limits.ProcessMemoryLimit = memory;
        limits.JobMemoryLimit = memory;
        let configured = unsafe {
            SetInformationJobObject(
                raw,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        };
        if configured == 0 || unsafe { AssignProcessToJobObject(raw, child.as_raw_handle()) } == 0 {
            return Err(format!(
                "解析プロセスを制限できません: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { _handle: handle })
    }
}

#[cfg(not(windows))]
pub struct ProcessJob;
#[cfg(not(windows))]
impl ProcessJob {
    pub fn attach(_: &Child, _: usize) -> Result<Self, String> {
        Ok(Self)
    }
}

pub fn spawn_limited(command: &mut Command, memory: usize) -> Result<(Child, ProcessJob), String> {
    hide_console(command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("処理を起動できません: {e}"))?;
    match ProcessJob::attach(&child, memory) {
        Ok(job) => Ok((child, job)),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

pub struct Captured {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn read_capped(reader: impl Read, limit: usize, exceeded: &AtomicBool) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        exceeded.store(true, Ordering::SeqCst);
        bytes.truncate(limit);
    }
    Ok(bytes)
}

pub fn capture(
    mut command: Command,
    timeout: Duration,
    limit: usize,
    stdin: Option<&[u8]>,
) -> Result<Captured, String> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let (mut child, _job) = spawn_limited(&mut command, ANALYSIS_MEMORY)?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let out = child.stdout.take().expect("piped stdout");
    let err = child.stderr.take().expect("piped stderr");
    let out_flag = exceeded.clone();
    let err_flag = exceeded.clone();
    let out_thread = thread::spawn(move || read_capped(out, limit, &out_flag));
    let err_thread = thread::spawn(move || read_capped(err, limit, &err_flag));
    // The C2PA worker waits here until its job limits are installed.
    let input_failed = Arc::new(AtomicBool::new(false));
    let input_thread = stdin.map(|bytes| {
        let bytes = bytes.to_vec();
        let mut pipe = child.stdin.take().expect("piped stdin");
        let failed = input_failed.clone();
        thread::spawn(move || {
            let result = pipe.write_all(&bytes);
            if result.is_err() {
                failed.store(true, Ordering::SeqCst);
            }
            result
        })
    });
    let started = Instant::now();
    let result = loop {
        if input_failed.load(Ordering::SeqCst) {
            break Err("解析データを渡せません。".to_string());
        }
        if exceeded.load(Ordering::SeqCst) {
            break Err("解析結果が上限を超えました。".to_string());
        }
        if started.elapsed() >= timeout {
            break Err("解析の制限時間を超えました。".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => break Err(format!("解析状態を確認できません: {error}")),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // Closing the job also terminates descendants that could keep a pipe open.
    drop(_job);
    if let Some(input) = input_thread {
        let _ = input.join();
    }
    let stdout = out_thread
        .join()
        .map_err(|_| "解析出力の取得に失敗しました。")?
        .map_err(|e| format!("解析出力を読み取れません: {e}"))?;
    let stderr = err_thread
        .join()
        .map_err(|_| "解析出力の取得に失敗しました。")?
        .map_err(|e| format!("解析出力を読み取れません: {e}"))?;
    if exceeded.load(Ordering::SeqCst) {
        return Err("解析結果が上限を超えました。".to_string());
    }
    Ok(Captured {
        status: result?,
        stdout,
        stderr,
    })
}

fn bounded_lines(reader: impl Read, mut on_line: impl FnMut(&str), exceeded: &AtomicBool) {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let mut total = 0usize;
    loop {
        let available = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(_) => {
                exceeded.store(true, Ordering::SeqCst);
                return;
            }
        };
        if available.is_empty() {
            if !line.is_empty() {
                on_line(&String::from_utf8_lossy(&line));
            }
            return;
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |p| p + 1);
        total += take;
        if line.len() + take > MAX_LINE || total > MAX_ENCODE_OUTPUT {
            exceeded.store(true, Ordering::SeqCst);
            return;
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if line.ends_with(b"\n") {
            on_line(String::from_utf8_lossy(&line).trim_end());
            line.clear();
        }
    }
}

pub fn encode(
    mut command: Command,
    process: &crate::state::ProcessControl,
    on_line: Arc<dyn Fn(bool, &str) + Send + Sync>,
) -> Result<(), String> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let (mut child, job) = spawn_limited(&mut command, 2 * 1024 * 1024 * 1024)?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let exceeded = Arc::new(AtomicBool::new(false));
    {
        let mut active = process
            .child
            .lock()
            .map_err(|_| "プロセス管理を開始できません。")?;
        if process.shutting_down.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("アプリを終了しています。".to_string());
        }
        *active = Some(child);
    }
    let out_callback = on_line.clone();
    let out_flag = exceeded.clone();
    let out_thread =
        thread::spawn(move || bounded_lines(stdout, |line| out_callback(false, line), &out_flag));
    let err_flag = exceeded.clone();
    let err_thread = thread::spawn(move || {
        let mut tail = VecDeque::new();
        bounded_lines(
            stderr,
            |line| {
                on_line(true, line);
                if tail.len() == 24 {
                    tail.pop_front();
                }
                tail.push_back(line.to_string());
            },
            &err_flag,
        );
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });
    let started = Instant::now();
    let result = loop {
        if exceeded.load(Ordering::SeqCst) {
            break Err("処理の出力量が安全上の上限を超えました。".to_string());
        }
        if started.elapsed() >= ENCODE_TIMEOUT {
            break Err("動画処理の制限時間（24時間）を超えました。".to_string());
        }
        let state = match process.child.lock() {
            Ok(mut active) => match active.as_mut() {
                Some(child) => child
                    .try_wait()
                    .map_err(|e| format!("処理状態を確認できません: {e}")),
                None => break Err("動画処理が中断されました。".to_string()),
            },
            Err(_) => break Err("プロセス管理を取得できません。".to_string()),
        };
        match state {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(e) => break Err(e),
        }
    };
    if let Ok(mut active) = process.child.lock() {
        if let Some(mut child) = active.take() {
            if result.is_err() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
    drop(job);
    let _ = out_thread.join();
    let details = err_thread
        .join()
        .map_err(|_| "処理結果を取得できません。")?;
    if exceeded.load(Ordering::SeqCst) {
        return Err("処理の出力量が安全上の上限を超えました。".to_string());
    }
    if result?.success() {
        Ok(())
    } else {
        Err(format!(
            "FFmpegがエラーを返しました。\n{}",
            details.chars().take(4000).collect::<String>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_output_without_a_newline_before_allocating_unbounded_memory() {
        let exceeded = AtomicBool::new(false);
        let input = vec![b'x'; MAX_LINE + 1];
        bounded_lines(
            input.as_slice(),
            |_| panic!("oversized line delivered"),
            &exceeded,
        );
        assert!(exceeded.load(Ordering::SeqCst));
    }

    #[test]
    fn caps_capture_including_the_final_chunk() {
        let exceeded = AtomicBool::new(false);
        assert_eq!(read_capped(&b"12345"[..], 4, &exceeded).unwrap(), b"1234");
        assert!(exceeded.load(Ordering::SeqCst));
    }

    #[test]
    fn accepts_a_permitted_unicode_xmp_field_within_the_line_limit() {
        let field = format!("  xmp: {}\n", "🟦".repeat(65_535));
        let exceeded = AtomicBool::new(false);
        let mut count = 0;
        bounded_lines(
            field.as_bytes(),
            |line| {
                assert_eq!(line, field.trim_end());
                count += 1;
            },
            &exceeded,
        );
        assert_eq!(count, 1);
        assert!(!exceeded.load(Ordering::SeqCst));
    }

    #[cfg(windows)]
    #[test]
    fn kills_a_process_that_exceeds_its_deadline() {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 10",
        ]);
        let started = Instant::now();
        let error = capture(command, Duration::from_millis(150), MAX_CAPTURE, None)
            .err()
            .expect("timeout");
        assert!(error.contains("制限時間"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(windows)]
    #[test]
    fn deadline_also_covers_a_worker_that_never_reads_stdin() {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 10",
        ]);
        let started = Instant::now();
        let error = capture(
            command,
            Duration::from_millis(150),
            MAX_CAPTURE,
            Some(&vec![b'x'; 128 * 1024]),
        )
        .err()
        .expect("timeout while pipe is blocked");
        assert!(error.contains("制限時間"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
