use super::*;
#[cfg(unix)]
#[test]
fn external_extractors_return_decoded_pixels_and_cleanup_temporary_files() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let png = f.0.join("output.png");
    image::RgbaImage::from_pixel(2, 3, image::Rgba([3, 7, 9, 255]))
        .save(&png)
        .unwrap();
    let ffmpeg = f.0.join("ffmpeg");
    let poppler = f.0.join("pdftoppm");
    std::fs::write(
        &ffmpeg,
        format!("#!/bin/sh\n/bin/cat '{}'\nexit 1\n", png.display()),
    )
    .unwrap();
    std::fs::write(
        &poppler,
        format!(
            "#!/bin/sh\nfor arg; do final=$arg; done\n/bin/cp '{}' \"$final.png\"\n",
            png.display()
        ),
    )
    .unwrap();
    for p in [&ffmpeg, &poppler] {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut options = f.options();
    options.process_timeout = std::time::Duration::from_secs(2);
    options.ffmpeg = ffmpeg;
    options.pdftoppm = poppler;
    for name in ["video.mp4", "photo.heic", "document.pdf"] {
        let request = f.request(name);
        std::fs::write(&request.path, b"fixture").unwrap();
        let PreviewContent::Pixels(p) = load_preview(&request, &options).unwrap() else {
            panic!("pixels")
        };
        assert_eq!((p.width, p.height), (2, 3));
        assert_eq!(&p.rgba[..4], &[3, 7, 9, 255]);
    }
    assert!(std::fs::read_dir(&f.0).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("ira_preview_")));
}
#[cfg(unix)]
#[test]
fn real_ffmpeg_video_fixture_when_explicit_test_tool_is_supplied() {
    // Generic suite verifies injected process adapters above. This opt-in native
    // capability probe is counted separately in the evidence when tool supplied.
    let Some(tool) = std::env::var_os("IRA_TEST_FFMPEG") else {
        return;
    };
    let f = Fixture::new();
    let request = f.request("actual.mp4");
    let status = std::process::Command::new(&tool)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=16x16:d=0.1",
            "-frames:v",
            "1",
            "-c:v",
            "mpeg4",
        ])
        .arg(&request.path)
        .status()
        .unwrap();
    assert!(status.success());
    let mut options = f.options();
    options.ffmpeg = tool.into();
    options.process_timeout = std::time::Duration::from_secs(10);
    let PreviewContent::Pixels(p) = load_preview(&request, &options).unwrap() else {
        panic!("pixels")
    };
    assert_eq!((p.width, p.height), (16, 16));
    assert!(p.rgba[0] > 200);
    assert!(p.rgba[1] < 30);
    assert_eq!(p.rgba[3], 255);
}
