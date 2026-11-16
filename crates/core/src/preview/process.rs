use super::*;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicU64;
use std::time::Instant;
struct TempOutput {
    path: PathBuf,
    directory: PathBuf,
}
impl TempOutput {
    fn new(options: &PreviewOptions, label: &str) -> Result<Self, PreviewError> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for _ in 0..64 {
            let directory = options.temp_dir.join(format!(
                "ira_preview_{label}_{}_{stamp:x}_{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&directory) {
                Ok(()) => {
                    return Ok(Self {
                        path: directory.join("frame.png"),
                        directory,
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(PreviewError::Failed(
            "preview staging names exhausted".into(),
        ))
    }
}
impl Drop for TempOutput {
    fn drop(&mut self) {
        // Only our output in an exclusively-created private namespace; never
        // recursively remove foreign entries or a colliding staging directory.
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}
/// ffmpeg retains the child for the advertised timeout. PDF preserves the
/// legacy watchdog ownership defect G0009 until an explicit parity decision.
pub(super) fn extract(
    request: &PreviewRequest,
    options: &PreviewOptions,
    pdf: bool,
) -> Result<image::DynamicImage, PreviewError> {
    if pdf {
        return legacy_pdf(request, options);
    }
    let output = TempOutput::new(options, "ffmpeg")?;
    let mut command = Command::new(&options.ffmpeg);
    command.stdin(Stdio::null()).stderr(Stdio::null());
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output.path)?;
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(&request.path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale='min(iw,512)':-2",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdout(Stdio::from(file));
    let mut child = command.spawn()?;
    let started = Instant::now();
    let result = loop {
        if request.cancellation.cancelled() {
            break Err(PreviewError::Cancelled);
        }
        if started.elapsed() >= options.process_timeout {
            break Err(PreviewError::Failed("ffmpeg timed out".into()));
        }
        match child.try_wait() {
            // Source ffmpeg path ignores status and accepts any valid frame output.
            Ok(Some(_)) => break Ok(()),
            Ok(None) => std::thread::sleep(JOB_POLL_INTERVAL),
            Err(e) => break Err(e.into()),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result?;
    let metadata = std::fs::metadata(&output.path)?;
    if metadata.len() == 0 {
        return Err(PreviewError::Failed("ffmpeg produced no frame".into()));
    }
    if metadata.len() > DECODE_MAX_ALLOC {
        return Err(PreviewError::Failed(
            "preview process output exceeds decode budget".into(),
        ));
    }
    Ok(cache::decode_source(&output.path)?)
}

// Source-preserving PDF ownership: take() before wait() removes the child from
// the watchdog. This known G0009 defect is intentionally unresolved here.
fn legacy_pdf(
    request: &PreviewRequest,
    options: &PreviewOptions,
) -> Result<image::DynamicImage, PreviewError> {
    use std::sync::Mutex;
    let output = TempOutput::new(options, "pdf")?;
    let root = output.path.with_extension("");
    let child = Command::new(&options.pdftoppm)
        .args([
            "-png",
            "-f",
            "1",
            "-l",
            "1",
            "-scale-to",
            "512",
            "-singlefile",
        ])
        .arg(&request.path)
        .arg(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let shared = Arc::new(Mutex::new(Some(child)));
    let watchdog = shared.clone();
    let timeout = options.process_timeout;
    std::thread::spawn(move || {
        for _ in 0..(timeout.as_millis() / 200) {
            std::thread::sleep(Duration::from_millis(200));
            if watchdog.lock().expect("watchdog poisoned").is_none() {
                return;
            }
        }
        if let Some(mut child) = watchdog.lock().expect("watchdog poisoned").take() {
            let _ = child.kill();
        }
    });
    if let Some(mut child) = shared.lock().expect("pdf child poisoned").take() {
        if !child.wait().map(|status| status.success()).unwrap_or(false) {
            return Err(PreviewError::Failed("pdftoppm failed".into()));
        }
    }
    let length = std::fs::metadata(&output.path)?.len();
    if length == 0 {
        return Err(PreviewError::Failed("pdftoppm produced no output".into()));
    }
    if length > DECODE_MAX_ALLOC {
        return Err(PreviewError::Failed(
            "preview process output exceeds decode budget".into(),
        ));
    }
    Ok(cache::decode_source(&output.path)?)
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
