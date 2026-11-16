use super::*;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::fs;
use std::io::BufReader;

pub(super) fn prune_cache(options: &PreviewOptions) {
    let Some(dir) = &options.cache_dir else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, e.path()))
        })
        .collect();
    if files.len() <= CACHE_MAX_FILES {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    let excess = files.len() - CACHE_MAX_FILES;
    for (_, path) in files.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
}
fn decode_oriented(
    reader: ImageReader<BufReader<fs::File>>,
    output_budget: u64,
) -> Result<DynamicImage, image::ImageError> {
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    let rgba_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4));
    if decoder.total_bytes() > output_budget || rgba_bytes.is_none_or(|bytes| bytes > output_budget)
    {
        return Err(image::ImageError::Limits(
            image::error::LimitError::from_kind(image::error::LimitErrorKind::InsufficientMemory),
        ));
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(img)
}
pub(super) fn decode_source(path: &Path) -> Result<DynamicImage, image::ImageError> {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(DECODE_MAX_ALLOC);
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    reader.limits(limits);
    decode_oriented(reader, DECODE_MAX_ALLOC)
}
pub(super) fn load_thumbnail(
    request: &PreviewRequest,
    options: &PreviewOptions,
) -> Result<DynamicImage, PreviewError> {
    let key = cache_key_path(&request.path, request.mtime, request.size);
    let cache_path = options
        .cache_dir
        .as_ref()
        .map(|dir| dir.join(format!("{key}.png")));
    if let Some(path) = &cache_path {
        if let Ok(img) = decode_source(path) {
            return Ok(img);
        }
    }
    let img = match preview_kind(&request.path.to_string_lossy()) {
        Some(PreviewKind::Video | PreviewKind::Heic) => process::extract(request, options, false)?,
        Some(PreviewKind::Pdf) => process::extract(request, options, true)?,
        _ => decode_source(&request.path)?,
    };
    if request.cancellation.cancelled() {
        return Err(PreviewError::Cancelled);
    }
    let thumb = if img.width() <= THUMB_MAX_PX && img.height() <= THUMB_MAX_PX {
        img
    } else {
        img.thumbnail(THUMB_MAX_PX, THUMB_MAX_PX)
    };
    if let Some(path) = &cache_path {
        store_thumbnail(&thumb, path);
    }
    Ok(thumb)
}
fn store_thumbnail(img: &DynamicImage, dest: &Path) {
    let Some(parent) = dest.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    // Independent temp entries avoid concurrent workers tearing the same cached PNG.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = dest.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    store_at(img, dest, &tmp);
}
fn store_at(img: &DynamicImage, dest: &Path, tmp: &Path) {
    let Ok(mut file) = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)
    else {
        return;
    };
    let encoded = img.write_to(&mut file, image::ImageFormat::Png);
    drop(file);
    if encoded.is_ok() {
        let _ = fs::rename(tmp, dest);
    }
    // Cleanup follows only our successful exclusive creation.
    let _ = fs::remove_file(tmp);
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
