//! A separate native window file, read and written only by one background owner.
use gpui::{Bounds, Pixels, Point, WindowBounds, px, size};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
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
enum Request {
    Load(mpsc::Sender<Option<Geometry>>),
    Save(Geometry),
    Barrier(mpsc::Sender<()>),
}
#[derive(Clone)]
pub struct Writer {
    sender: mpsc::Sender<Request>,
    errors: Arc<Mutex<mpsc::Receiver<String>>>,
}
impl Writer {
    pub fn new(path: Option<PathBuf>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let (error_tx, errors) = mpsc::channel();
        thread::spawn(move || {
            let mut current = None;
            let mut loaded = false;
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
                        current = Some(value);
                        loaded = true;
                        if let Some(path) = &path
                            && let Err(error) = save(path, value)
                        {
                            let _ =
                                error_tx.send(format!("Window state could not be saved: {error}"));
                        }
                    }
                    Request::Barrier(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        });
        Self {
            sender,
            errors: Arc::new(Mutex::new(errors)),
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
    pub fn barrier(&self) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        let _ = self.sender.send(Request::Barrier(tx));
        rx
    }
    pub fn try_error(&self) -> Option<String> {
        self.errors.try_lock().ok()?.try_recv().ok()
    }
}
pub fn path() -> Option<PathBuf> {
    ira_core::theme::theme_file_path().map(|path| path.with_file_name("desktop-window"))
}
