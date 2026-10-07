use super::preview::*;
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "ira-preview-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn options(&self) -> PreviewOptions {
        PreviewOptions {
            cache_dir: Some(self.0.join("cache")),
            temp_dir: self.0.clone(),
            ffmpeg: self.0.join("missing-ffmpeg"),
            pdftoppm: self.0.join("missing-poppler"),
            process_timeout: std::time::Duration::from_millis(40),
        }
    }
    fn request(&self, name: &str) -> PreviewRequest {
        PreviewRequest {
            session_id: 1,
            pane: 0,
            generation: 7,
            path: self.0.join(name),
            mtime: Some(123),
            size: 0,
            surface: PreviewSurface::Column,
            cancellation: Cancellation::default(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn classification_and_cache_key_match_frozen_source_rules() {
    assert_eq!(preview_kind("photo.JPEG"), Some(PreviewKind::Image));
    assert_eq!(preview_kind("README.old"), Some(PreviewKind::Text));
    assert_eq!(preview_kind(".env.local"), Some(PreviewKind::Text));
    assert_eq!(preview_kind("Makefile"), Some(PreviewKind::Text));
    assert_eq!(preview_kind("video.mkv"), Some(PreviewKind::Video));
    assert_eq!(preview_kind("photo.heic"), Some(PreviewKind::Heic));
    assert_eq!(preview_kind("book.pdf"), Some(PreviewKind::Pdf));
    assert_eq!(preview_kind("unknown.zqq"), None);
    fn oracle_hash(path: &str, mtime: Option<i64>, size: u64) -> String {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in format!("{path}\0{}\0{size}", mtime.unwrap_or(-1)).bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{h:016x}")
    }
    for path in ["/a é", "/b\0x"] {
        assert_eq!(cache_key(path, None, 123), oracle_hash(path, None, 123));
        assert_eq!(cache_key(path, Some(0), 0), oracle_hash(path, Some(0), 0));
    }
}
#[test]
fn text_head_exact_cap_nul_lossy_empty_and_truncation() {
    let f = Fixture::new();
    let p = f.0.join("file.txt");
    std::fs::write(&p, []).unwrap();
    assert_eq!(read_text_preview(&p).unwrap().content, "");
    std::fs::write(&p, [b'a', 0, 255]).unwrap();
    let t = read_text_preview(&p).unwrap();
    assert!(t.binary);
    assert_eq!(t.content, "a\0�");
    assert!(!t.truncated);
    let mut b = vec![b'x'; TEXT_PREVIEW_CAP as usize + 1];
    b[TEXT_PREVIEW_CAP as usize] = 0;
    std::fs::write(&p, &b).unwrap();
    let t = read_text_preview(&p).unwrap();
    assert!(t.truncated);
    assert!(!t.binary);
    assert_eq!(t.content.len(), TEXT_PREVIEW_CAP as usize);
}
#[test]
fn real_rgba_decode_content_sniffing_cache_recovery_and_downscale() {
    let f = Fixture::new();
    let request = f.request("image.dat");
    image::RgbaImage::from_pixel(1000, 500, image::Rgba([17, 23, 42, 255]))
        .save_with_format(&request.path, image::ImageFormat::Png)
        .unwrap();
    let options = f.options();
    let PreviewContent::Pixels(pixels) = load_preview(&request, &options).unwrap() else {
        panic!("pixels")
    };
    assert!(pixels.width <= THUMB_MAX_PX);
    assert_eq!(pixels.width, 2 * pixels.height);
    assert_eq!(&pixels.rgba[..4], &[17, 23, 42, 255]);
    assert_eq!(
        pixels.rgba.len(),
        (pixels.width * pixels.height * 4) as usize
    );
    let cached = options.cache_dir.as_ref().unwrap().join(format!(
        "{}.png",
        cache_key(&request.path.to_string_lossy(), request.mtime, request.size)
    ));
    std::fs::write(&cached, b"corrupt").unwrap();
    assert!(load_preview(&request, &options).is_ok());
    std::fs::remove_file(&request.path).unwrap();
    assert!(load_preview(&request, &options).is_ok());
}
#[test]
fn cancellation_and_stale_envelope_are_explicit_and_worker_counts_bounded() {
    let f = Fixture::new();
    let request = f.request("file.txt");
    std::fs::write(&request.path, b"abc").unwrap();
    assert!(request.matches(1, 0, 7, &request.path));
    assert!(!request.matches(1, 0, 8, &request.path));
    request.cancellation.cancel();
    assert_eq!(
        load_preview(&request, &f.options()).unwrap_err(),
        PreviewError::Cancelled
    );
    let pool = PreviewPool::with_workers(f.options(), 99);
    assert_eq!(pool.worker_count(), MAX_DECODE_THREADS);
    let req = f.request("file.txt");
    pool.submit(req, true).unwrap();
    let event = pool
        .events
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    assert!(event.request.matches(1, 0, 7, &event.request.path));
    assert_eq!(
        event.result.unwrap(),
        PreviewContent::Text(TextPreview {
            content: "abc".into(),
            binary: false,
            truncated: false
        })
    );
}
#[test]
fn missing_external_tools_and_invalid_images_return_real_failures() {
    let f = Fixture::new();
    for name in ["file.pdf", "file.mp4", "file.heic", "file.png"] {
        let req = f.request(name);
        std::fs::write(&req.path, b"invalid").unwrap();
        assert!(matches!(
            load_preview(&req, &f.options()),
            Err(PreviewError::Failed(_))
        ));
    }
}

#[test]
fn classification_and_cache_identity_are_differential_against_source() {
    for path in [
        "photo.png",
        "UPPER.MKV",
        "photo.heif",
        ".env.local",
        "Dockerfile.dev",
        "unknown.zqq",
        "CMakeLists.txt",
        "LICENSE",
        "f.pdf",
        "f.tsv",
    ] {
        assert_eq!(
            format!("{:?}", preview_kind(path)),
            format!("{:?}", oracle::preview_kind(path))
        );
        assert_eq!(
            cache_key(path, Some(-1), 234),
            oracle::cache_key(path, Some(-1), 234)
        );
    }
}
#[test]
fn cache_prunes_only_png_and_keeps_pinned_budget() {
    let f = Fixture::new();
    let options = f.options();
    let cache = options.cache_dir.as_ref().unwrap();
    std::fs::create_dir(cache).unwrap();
    for i in 0..CACHE_MAX_FILES + 2 {
        std::fs::write(cache.join(format!("{i}.png")), []).unwrap();
    }
    std::fs::write(cache.join("keep.txt"), []).unwrap();
    prune_cache(&options);
    assert_eq!(
        std::fs::read_dir(cache)
            .unwrap()
            .filter(|p| p
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "png"))
            .count(),
        CACHE_MAX_FILES
    );
    assert!(cache.join("keep.txt").exists());
}
#[test]
fn exif_orientation_rotates_decoded_dimensions() {
    let f = Fixture::new();
    let request = f.request("rotated.jpg");
    image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30]))
        .save(&request.path)
        .unwrap();
    let bytes = std::fs::read(&request.path).unwrap();
    let exif = [
        b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0,
        0, 6, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut out = vec![0xff, 0xd8, 0xff, 0xe1, 0, 34];
    out.extend(exif);
    out.extend_from_slice(&bytes[2..]);
    std::fs::write(&request.path, out).unwrap();
    let PreviewContent::Pixels(p) = load_preview(&request, &f.options()).unwrap() else {
        panic!("pixels")
    };
    assert_eq!((p.width, p.height), (2, 3));
}
#[cfg(unix)]
#[test]
fn source_pdf_watchdog_loses_child_ownership_before_wait_safely_reproduced() {
    use std::sync::{Arc, Mutex};
    let child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let shared = Arc::new(Mutex::new(Some(child)));
    // This is the exact ordering at source thumbnails.rs417: take, then wait.
    let mut waiting_child = shared.lock().unwrap().take().unwrap();
    let watchdog = shared.clone();
    let ownership = std::thread::spawn(move || watchdog.lock().unwrap().is_none())
        .join()
        .unwrap();
    assert!(
        ownership,
        "source watchdog returns because its Child Option is None"
    );
    assert!(waiting_child.try_wait().unwrap().is_none());
    waiting_child.kill().unwrap();
    waiting_child.wait().unwrap();
}
#[cfg(unix)]
#[test]
fn native_non_utf8_paths_do_not_collide_in_cache_identity() {
    use std::os::unix::ffi::OsStringExt;
    let a = PathBuf::from(std::ffi::OsString::from_vec(vec![b'a', 255]));
    let b = PathBuf::from(std::ffi::OsString::from_vec(vec![b'a', 254]));
    assert_eq!(a.to_string_lossy(), b.to_string_lossy());
    assert_ne!(cache_key_path(&a, None, 0), cache_key_path(&b, None, 0));
    assert_eq!(
        cache_key_path(std::path::Path::new("/ascii"), None, 0),
        cache_key("/ascii", None, 0)
    );
}
#[test]
fn characterize_source_jpeg_output_ignores_configured_max_alloc_without_reserve() {
    // Source thumbnails::decode_oriented uses into_decoder -> from_decoder.
    // Use a one-byte budget and a 1x1 JPEG to prove the bypass without a large allocation.
    use image::ImageDecoder;
    let f = Fixture::new();
    let p = f.0.join("one.jpg");
    image::RgbImage::from_pixel(1, 1, image::Rgb([1, 2, 3]))
        .save(&p)
        .unwrap();
    let mut reader = image::ImageReader::open(&p)
        .unwrap()
        .with_guessed_format()
        .unwrap();
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(1);
    reader.limits(limits);
    let decoder = reader.into_decoder().unwrap();
    assert_eq!(decoder.total_bytes(), 3);
    let image = image::DynamicImage::from_decoder(decoder).unwrap();
    assert_eq!(image.as_bytes().len(), 3);
}

#[path = "oracle_classify.rs"]
mod oracle;

#[path = "extractor_tests.rs"]
mod extractors;
