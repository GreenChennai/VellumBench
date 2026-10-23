//! 项目内位图资源加载。路径仅限项目根目录，远程 URL 不联网读取。

use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{ImageReader, Limits};

use crate::encode::BitmapData;

/// Maximum encoded raster size accepted by project import and insertion.
pub const MAX_PROJECT_IMAGE_SOURCE_BYTES: usize = 32 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = MAX_PROJECT_IMAGE_SOURCE_BYTES;
const MAX_SOURCE_FILE_BYTES: u64 = MAX_SOURCE_BYTES as u64;
const MAX_DIMENSION: u32 = 8192;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;

/// 从项目根目录安全加载相对位图路径或 `data:image/*;base64,...`。
///
/// 拒绝远程协议、路径越界、超大输入以及超过解码尺寸预算的图像。
pub fn load_project_bitmap(project_root: &Path, src: &str) -> Option<BitmapData> {
    let source = src.trim();
    if source.is_empty() {
        return None;
    }

    let (bytes, expected_extension) = if source
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
    {
        decode_data_url(source)?
    } else {
        let path = vb_common::project_asset::resolve_project_asset(project_root, source)?;
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        let expected_extension = normalize_supported_extension(&extension)?;
        let mut file = fs::File::open(path).ok()?;
        let size = file.metadata().ok()?.len();
        if size == 0 || size > MAX_SOURCE_FILE_BYTES {
            return None;
        }
        let mut bytes = Vec::with_capacity(size.min(MAX_SOURCE_FILE_BYTES) as usize);
        file.by_ref()
            .take(MAX_SOURCE_FILE_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > MAX_SOURCE_BYTES {
            return None;
        }
        (bytes, expected_extension)
    };

    let (bitmap, actual_extension) = decode_project_bitmap_with_extension(&bytes)?;
    (actual_extension == expected_extension).then_some(bitmap)
}

/// Decode a bounded raster byte slice using the same dimension/allocation policy
/// as project-relative assets. Used when ingesting a file selected by the host.
pub fn decode_project_bitmap(bytes: &[u8]) -> Option<BitmapData> {
    decode_project_bitmap_with_extension(bytes).map(|(bitmap, _)| bitmap)
}

/// Decode a project raster and return its canonical file extension.
pub fn decode_project_bitmap_with_extension(bytes: &[u8]) -> Option<(BitmapData, &'static str)> {
    decode_bitmap_with_extension(bytes)
}

fn decode_data_url(src: &str) -> Option<(Vec<u8>, &'static str)> {
    let rest = src.get(..)?.get(5..)?;
    let (metadata, encoded) = rest.split_once(',')?;
    let mut parts = metadata.split(';');
    let mime = parts.next()?.trim().to_ascii_lowercase();
    let expected_extension = match mime.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tif",
        _ => return None,
    };
    if !parts.any(|part| part.eq_ignore_ascii_case("base64")) {
        return None;
    }
    let max_encoded = MAX_SOURCE_BYTES.div_ceil(3).checked_mul(4)?;
    if encoded.len() > max_encoded {
        return None;
    }
    let bytes = STANDARD.decode(encoded).ok()?;
    (bytes.len() <= MAX_SOURCE_BYTES).then_some((bytes, expected_extension))
}

fn normalize_supported_extension(extension: &str) -> Option<&'static str> {
    match extension {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "webp" => Some("webp"),
        "bmp" => Some("bmp"),
        "tif" | "tiff" => Some("tif"),
        _ => None,
    }
}

fn decode_bitmap_with_extension(bytes: &[u8]) -> Option<(BitmapData, &'static str)> {
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES {
        return None;
    }

    let probe = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let format = probe.format()?;
    let extension = match format {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Bmp => "bmp",
        image::ImageFormat::Tiff => "tif",
        _ => return None,
    };
    let (width, height) = probe.into_dimensions().ok()?;
    let pixels = u64::from(width).checked_mul(u64::from(height))?;
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || pixels > MAX_PIXELS
    {
        return None;
    }

    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.to_rgba8();
    Some((
        BitmapData {
            width: rgba.width(),
            height: rgba.height(),
            rgba: Arc::new(rgba.into_raw()),
        },
        extension,
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Cursor,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    use super::load_project_bitmap;

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "vb-render-assets-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create isolated asset fixture");
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tiny_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255])));
        let mut output = Cursor::new(Vec::new());
        image
            .write_to(&mut output, ImageFormat::Png)
            .expect("encode fixture PNG");
        output.into_inner()
    }

    #[test]
    fn loads_project_relative_and_base64_data_images() {
        let root = TempRoot::new();
        let assets = root.0.join("assets");
        fs::create_dir_all(&assets).expect("create assets directory");
        let bytes = tiny_png();
        fs::write(assets.join("pixel.png"), &bytes).expect("write fixture PNG");

        let local = load_project_bitmap(&root.0, "assets/pixel.png?revision=1").unwrap();
        assert_eq!((local.width, local.height), (1, 1));
        assert_eq!(local.rgba.as_slice(), &[255, 0, 0, 255]);

        let data_url = format!("data:image/png;base64,{}", STANDARD.encode(bytes));
        let inline = load_project_bitmap(&root.0, &data_url).unwrap();
        assert_eq!((inline.width, inline.height), (1, 1));
        assert_eq!(inline.rgba.as_slice(), &[255, 0, 0, 255]);
    }

    #[test]
    fn rejects_mismatched_file_extension_and_data_mime() {
        let root = TempRoot::new();
        let assets = root.0.join("assets");
        fs::create_dir_all(&assets).expect("create assets directory");
        let png = tiny_png();
        fs::write(assets.join("pixel.jpg"), &png).expect("write mismatched PNG payload");

        assert!(load_project_bitmap(&root.0, "assets/pixel.jpg").is_none());
        let data_url = format!("data:image/jpeg;base64,{}", STANDARD.encode(png));
        assert!(load_project_bitmap(&root.0, &data_url).is_none());
    }

    #[test]
    fn rejects_remote_and_project_escape_paths() {
        let root = TempRoot::new();
        let outside = root.0.join("outside.png");
        fs::write(&outside, tiny_png()).expect("write outside fixture");
        let project = root.0.join("project");
        fs::create_dir_all(&project).expect("create project root");

        assert!(load_project_bitmap(&project, "../outside.png").is_none());
        assert!(load_project_bitmap(&project, "%2e%2e/outside.png").is_none());
        assert!(load_project_bitmap(&project, outside.to_str().unwrap()).is_none());
        assert!(load_project_bitmap(&project, "https://example.invalid/image.png").is_none());
        assert!(load_project_bitmap(&project, "data:text/plain;base64,SGVsbG8=").is_none());
    }
}
