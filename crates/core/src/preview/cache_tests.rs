use super::*;
#[test]
fn native_decoder_checks_total_output_before_allocating_with_tiny_fixture_budget() {
    let path = std::env::temp_dir().join(format!("ira-jpeg-budget-{}.jpg", std::process::id()));
    image::RgbImage::from_pixel(1, 1, image::Rgb([1, 2, 3]))
        .save(&path)
        .unwrap();
    let reader = ImageReader::open(&path)
        .unwrap()
        .with_guessed_format()
        .unwrap();
    assert!(matches!(
        decode_oriented(reader, 1),
        Err(image::ImageError::Limits(_))
    ));
    // RGB decoder requires three bytes, but the published RGBA pixels need four.
    let reader = ImageReader::open(&path)
        .unwrap()
        .with_guessed_format()
        .unwrap();
    assert!(matches!(
        decode_oriented(reader, 3),
        Err(image::ImageError::Limits(_))
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn cache_collision_neither_overwrites_nor_removes_unowned_staging() {
    let directory =
        std::env::temp_dir().join(format!("ira-cache-collision-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let dest = directory.join("cache.png");
    let tmp = directory.join("foreign.tmp");
    std::fs::write(&tmp, b"foreign").unwrap();
    std::fs::write(&dest, b"existing cache").unwrap();
    let image = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([1, 2, 3, 255]),
    ));
    store_at(&image, &dest, &tmp);
    assert_eq!(std::fs::read(&tmp).unwrap(), b"foreign");
    assert_eq!(std::fs::read(&dest).unwrap(), b"existing cache");
    std::fs::remove_dir_all(directory).unwrap();
}
