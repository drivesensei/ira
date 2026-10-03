//! A separate native window file, read and written only by one background owner.
use gpui::{Bounds, Pixels, Point, WindowBounds, px, size};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub mode: u8,
}
impl Geometry {
    pub fn parse(text: &str) -> Option<Self> {
        let fields: Vec<_> = text.split_whitespace().collect();
        if fields.len() != 6 || fields[0] != "ira-window-v1" {
            return None;
        }
        let value = Self {
            x: fields[1].parse().ok()?,
            y: fields[2].parse().ok()?,
            width: fields[3].parse().ok()?,
            height: fields[4].parse().ok()?,
            mode: fields[5].parse().ok()?,
        };
        if [value.x, value.y, value.width, value.height]
            .into_iter()
            .any(|v| !v.is_finite())
            || !(320.0..=16384.0).contains(&value.width)
            || !(200.0..=16384.0).contains(&value.height)
            || value.x.abs() > 1_000_000.
            || value.y.abs() > 1_000_000.
            || value.mode > 2
        {
            return None;
        }
        Some(value)
    }
    pub fn from_bounds(value: WindowBounds) -> Self {
        let mode = match value {
            WindowBounds::Windowed(_) => 0,
            WindowBounds::Maximized(_) => 1,
            WindowBounds::Fullscreen(_) => 2,
        };
        let b = value.get_bounds();
        Self {
            x: f32::from(b.origin.x),
            y: f32::from(b.origin.y),
            width: f32::from(b.size.width),
            height: f32::from(b.size.height),
            mode,
        }
    }
    pub fn restore(self, displays: &[Bounds<Pixels>], fallback: Bounds<Pixels>) -> WindowBounds {
        let mut bounds = Bounds {
            origin: Point::new(px(self.x), px(self.y)),
            size: size(px(self.width), px(self.height)),
        };
        // A visible titlebar region is required after a monitor is removed.
        let visible = displays.iter().any(|display| {
            self.x + 80. > f32::from(display.origin.x)
                && self.x < f32::from(display.right()) - 80.
                && self.y >= f32::from(display.origin.y)
                && self.y + 40. < f32::from(display.bottom())
        });
        if !visible {
            bounds = fallback;
        }
        match self.mode {
            1 => WindowBounds::Maximized(bounds),
            2 => WindowBounds::Fullscreen(bounds),
            _ => WindowBounds::Windowed(bounds),
        }
    }
    pub fn encode(self) -> String {
        format!(
            "ira-window-v1 {} {} {} {} {}\n",
            self.x, self.y, self.width, self.height, self.mode
        )
    }
}
fn save(path: &Path, value: Geometry) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Window state has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".ira-window-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(value.encode().as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
/// Receipt for geometry writes accepted before a checked barrier.
#[derive(Clone, Debug)]
pub struct Receipt {
    pub epoch: u64,
}
#[derive(Clone)]
pub struct Failure {
    pub epoch: u64,
    pub message: String,
    pub retry: Retry,
}
impl std::fmt::Debug for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeometryFailure")
            .field("epoch", &self.epoch)
            .field("message", &self.message)
            .finish_non_exhaustive()
    }
}
pub type Checked = Result<Receipt, Failure>;
#[derive(Clone)]
pub struct Retry {
    _owner: Arc<mpsc::Sender<Request>>,
    sender: mpsc::Sender<Request>,
    epoch: u64,
    value: Geometry,
    busy: Arc<AtomicBool>,
}
impl Retry {
    /// Retry the captured failed payload only; never capture current window state.
    pub fn retry(&self) -> mpsc::Receiver<Checked> {
        let (reply, receiver) = mpsc::channel();
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            let _ = reply.send(Err(Failure {
                epoch: self.epoch,
                message: "Geometry retry already pending".into(),
                retry: self.clone(),
            }));
            return receiver;
        }
        if let Err(error) = self.sender.send(Request::Retry {
            epoch: self.epoch,
            value: self.value,
            reply,
            retry: self.clone(),
        }) {
            self.busy.store(false, Ordering::Release);
            if let Request::Retry { reply, .. } = error.0 {
                let _ = reply.send(Err(Failure {
                    epoch: self.epoch,
                    message: "Geometry writer disconnected".into(),
                    retry: self.clone(),
                }));
            }
        }
        receiver
    }
}
enum Request {
    Load(mpsc::Sender<Option<Geometry>>),
    Save(Geometry),
    Barrier(mpsc::Sender<()>),
    CheckedBarrier(mpsc::Sender<Checked>),
    Retry {
        epoch: u64,
        value: Geometry,
        reply: mpsc::Sender<Checked>,
        retry: Retry,
    },
}
#[derive(Clone)]
pub struct Writer {
    _owner: Arc<mpsc::Sender<Request>>,
    sender: mpsc::Sender<Request>,
    errors: Arc<Mutex<mpsc::Receiver<String>>>,
}
impl Writer {
    pub fn new(path: Option<PathBuf>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let (error_tx, errors) = mpsc::channel();
        // A retained retry sender keeps this owner alive beyond Desktop destruction.
        let weak_sender = Arc::new(sender.clone());
        let retry_sender = Arc::downgrade(&weak_sender);
        let busy = Arc::new(AtomicBool::new(false));
        thread::spawn(move || {
            let mut current = None;
            let mut loaded = false;
            let mut epoch = 0;
            let mut failure: Option<String> = None;
            while let Ok(request) = receiver.recv() {
                match request {
                    Request::Load(reply) => {
                        if !loaded {
                            current = path
                                .as_ref()
                                .and_then(|path| fs::read_to_string(path).ok())
                                .and_then(|text| Geometry::parse(&text));
                            loaded = true;
                        }
                        let _ = reply.send(current);
                    }
                    Request::Save(value) => {
                        epoch += 1;
                        current = Some(value);
                        loaded = true;
                        failure = path
                            .as_ref()
                            .and_then(|path| save(path, value).err())
                            .map(|error| format!("Window state could not be saved: {error}"));
                        if let Some(error) = &failure {
                            let _ = error_tx.send(error.clone());
                        }
                    }
                    Request::Barrier(reply) => {
                        let _ = reply.send(());
                    }
                    Request::CheckedBarrier(reply) => {
                        let result = if let (Some(message), Some(value), Some(sender)) =
                            (&failure, current, retry_sender.upgrade())
                        {
                            Err(Failure {
                                epoch,
                                message: message.clone(),
                                retry: Retry {
                                    _owner: sender.clone(),
                                    sender: (*sender).clone(),
                                    epoch,
                                    value,
                                    busy: busy.clone(),
                                },
                            })
                        } else if failure.is_some() {
                            // The receiver disconnects honestly if ownership is already gone.
                            continue;
                        } else {
                            Ok(Receipt { epoch })
                        };
                        let _ = reply.send(result);
                    }
                    Request::Retry {
                        epoch: required,
                        value,
                        reply,
                        retry,
                    } => {
                        let result = if required != epoch {
                            Err(Failure {
                                epoch: required,
                                message: "Geometry retry superseded by newer accepted state".into(),
                                retry: retry.clone(),
                            })
                        } else if failure.is_none() {
                            Ok(Receipt { epoch })
                        } else {
                            failure = path
                                .as_ref()
                                .and_then(|path| save(path, value).err())
                                .map(|error| format!("Window state could not be saved: {error}"));
                            match &failure {
                                Some(message) => Err(Failure {
                                    epoch,
                                    message: message.clone(),
                                    retry: retry.clone(),
                                }),
                                None => Ok(Receipt { epoch }),
                            }
                        };
                        retry.busy.store(false, Ordering::Release);
                        let _ = reply.send(result);
                    }
                }
            }
        });
        Self {
            sender,
            errors: Arc::new(Mutex::new(errors)),
            _owner: weak_sender,
        }
    }
    pub fn load(&self) -> mpsc::Receiver<Option<Geometry>> {
        let (tx, rx) = mpsc::channel();
        let _ = self.sender.send(Request::Load(tx));
        rx
    }
    pub fn save(&self, value: Geometry) {
        let _ = self.sender.send(Request::Save(value));
    }
    /// Legacy processing-only barrier, retained for source compatibility.
    pub fn barrier(&self) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        let _ = self.sender.send(Request::Barrier(tx));
        rx
    }
    pub fn checked_barrier(&self) -> mpsc::Receiver<Checked> {
        let (tx, rx) = mpsc::channel();
        let _ = self.sender.send(Request::CheckedBarrier(tx));
        rx
    }
    pub fn try_error(&self) -> Option<String> {
        self.errors.try_lock().ok()?.try_recv().ok()
    }
}
pub fn path() -> Option<PathBuf> {
    ira_core::theme::theme_file_path().map(|path| path.with_file_name("desktop-window"))
}

#[cfg(test)]
mod checked_tests {
    use super::*;
    fn value(x: f32) -> Geometry {
        Geometry {
            x,
            y: 0.,
            width: 1080.,
            height: 720.,
            mode: 0,
        }
    }
    #[test]
    fn failed_geometry_receipt_survives_writer_and_retries_frozen_value() {
        let _scope = crate::test_support::enter();
        let fixture = crate::test_support::current().unwrap().directory.clone();
        let parent = fixture.join("blocked");
        fs::write(&parent, b"temporary obstruction").unwrap();
        let path = parent.join("desktop-window");
        let writer = Writer::new(Some(path.clone()));
        writer.save(value(42.));
        let original = writer.checked_barrier();
        let failure = original
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap_err();
        assert_eq!(failure.epoch, 1);
        drop(writer);
        fs::remove_file(&parent).unwrap();
        fs::create_dir(&parent).unwrap();
        let receipt = failure
            .retry
            .retry()
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap();
        assert_eq!(receipt.epoch, failure.epoch);
        assert_eq!(
            Geometry::parse(&fs::read_to_string(&path).unwrap())
                .unwrap()
                .x,
            42.
        );
        // A successful retry is idempotent, not a second filesystem publication.
        fs::remove_file(&path).unwrap();
        assert_eq!(
            failure
                .retry
                .retry()
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap()
                .unwrap()
                .epoch,
            1
        );
        assert!(!path.exists());
    }
    #[test]
    fn old_geometry_failure_cannot_overwrite_newer_success() {
        let _scope = crate::test_support::enter();
        let fixture = crate::test_support::current().unwrap().directory.clone();
        let parent = fixture.join("blocked");
        fs::write(&parent, b"temporary obstruction").unwrap();
        let path = parent.join("desktop-window");
        let writer = Writer::new(Some(path.clone()));
        writer.save(value(42.));
        let failure = writer
            .checked_barrier()
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap_err();
        fs::remove_file(&parent).unwrap();
        fs::create_dir(&parent).unwrap();
        writer.save(value(99.));
        let receipt = writer
            .checked_barrier()
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap();
        assert_eq!(receipt.epoch, 2);
        let stale = failure
            .retry
            .retry()
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap_err();
        assert!(stale.message.contains("superseded"));
        assert_eq!(
            Geometry::parse(&fs::read_to_string(&path).unwrap())
                .unwrap()
                .x,
            99.
        );
    }
}
