use super::*;
#[test]
fn private_process_staging_is_owned_and_cleanup_keeps_foreign_entries() {
    let parent =
        std::env::temp_dir().join(format!("ira-process-staging-test-{}", std::process::id()));
    std::fs::create_dir(&parent).unwrap();
    let options = PreviewOptions {
        cache_dir: None,
        temp_dir: parent.clone(),
        ffmpeg: "ffmpeg".into(),
        pdftoppm: "pdftoppm".into(),
        process_timeout: FFMPEG_TIMEOUT,
    };
    let output = TempOutput::new(&options, "fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&output.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    std::fs::write(&output.path, b"owned").unwrap();
    let directory = output.directory.clone();
    let foreign = directory.join("foreign");
    std::fs::write(&foreign, b"keep").unwrap();
    drop(output);
    assert_eq!(std::fs::read(&foreign).unwrap(), b"keep");
    assert!(!directory.join("frame.png").exists());
    std::fs::remove_dir_all(parent).unwrap();
}
