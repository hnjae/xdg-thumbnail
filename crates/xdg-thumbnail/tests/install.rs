// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, OriginalIdentity, ParsedThumbnailPng, PersonalCacheRoot, PersonalOriginalUri,
    RawThumbnailImage, RawThumbnailPixelFormat, ReadableOriginalIdentity, ThumbnailError,
    ThumbnailPngBitDepth, ThumbnailPngColorType, ThumbnailSize, UnixMTimeSeconds,
};

#[test]
fn installs_normalized_downscaled_personal_thumbnail_atomically() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let rendered = png_without_metadata(300, 150, png::ColorType::Rgb);

    let installed = root
        .install_thumbnail_png_bytes(&original, ThumbnailSize::Normal, &rendered)
        .unwrap();

    let expected_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    assert_eq!(installed.path(), expected_path.as_path());
    assert_eq!(
        std::fs::read(&expected_path).unwrap(),
        installed.png_bytes()
    );

    let parsed = ParsedThumbnailPng::parse(installed.png_bytes()).unwrap();
    assert_eq!(parsed.width(), 128);
    assert_eq!(parsed.height(), 64);
    assert_eq!(parsed.bit_depth(), ThumbnailPngBitDepth::Eight);
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
    assert!(!parsed.interlaced());
    assert_eq!(
        parsed.metadata().thumb_uri(),
        Some("file:///home/alice/photo.png")
    );
    assert_eq!(
        parsed.metadata().thumb_mtime(),
        Some(UnixMTimeSeconds::new(42))
    );
    assert_eq!(parsed.metadata().thumb_size(), Some(12));
    assert_eq!(parsed.metadata().thumb_mimetype(), Some("image/png"));

    let dir_mode = std::fs::metadata(expected_path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let file_mode = std::fs::metadata(&expected_path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);
    assert_eq!(file_mode, 0o600);
}

#[test]
fn path_install_variant_returns_only_installed_path() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let rendered = png_without_metadata(2, 1, png::ColorType::Rgba);

    let installed = root
        .install_thumbnail_path(&original, ThumbnailSize::Normal, &rendered)
        .unwrap();

    let expected_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    assert_eq!(installed.path(), expected_path.as_path());
    assert!(expected_path.exists());
}

#[test]
fn install_rejects_insecure_existing_cache_directories() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let target_dir = root.as_path().join("normal");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::set_permissions(&target_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let error = root
        .install_thumbnail_png_bytes(
            &readable_original(),
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1, png::ColorType::Rgba),
        )
        .unwrap_err();

    assert!(error.to_string().contains("insecure"));
}

#[test]
fn install_rejects_symlinked_cache_directories() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(root.as_path()).unwrap();
    symlink(&outside, root.as_path().join("normal")).unwrap();

    let error = root
        .install_thumbnail_png_bytes(
            &readable_original(),
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1, png::ColorType::Rgba),
        )
        .unwrap_err();

    assert!(error.to_string().contains("insecure"));
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
}

#[test]
fn installs_rgb8_raw_thumbnail_with_opaque_alpha() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let pixels = [10, 20, 30, 40, 50, 60];
    let image = RawThumbnailImage::new(2, 1, 6, RawThumbnailPixelFormat::Rgb8, &pixels).unwrap();

    let installed = root
        .install_thumbnail_raw_png_bytes(&original, ThumbnailSize::Normal, image)
        .unwrap();

    let expected_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    assert_eq!(installed.path(), expected_path.as_path());
    assert_eq!(
        std::fs::read(&expected_path).unwrap(),
        installed.png_bytes()
    );
    let (width, height, rgba) = decode_rgba(installed.png_bytes());
    assert_eq!((width, height), (2, 1));
    assert_eq!(rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
    assert_eq!(
        ParsedThumbnailPng::parse(installed.png_bytes())
            .unwrap()
            .metadata()
            .thumb_uri(),
        Some("file:///home/alice/photo.png")
    );
}

#[test]
fn installs_rgba8_raw_thumbnail_preserving_alpha() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let pixels = [10, 20, 30, 11, 40, 50, 60, 77];
    let image = RawThumbnailImage::new(2, 1, 8, RawThumbnailPixelFormat::Rgba8, &pixels).unwrap();

    let installed = root
        .install_thumbnail_raw_png_bytes(&original, ThumbnailSize::Normal, image)
        .unwrap();

    let (width, height, rgba) = decode_rgba(installed.png_bytes());
    assert_eq!((width, height), (2, 1));
    assert_eq!(rgba, pixels);
}

#[test]
fn raw_thumbnail_stride_padding_is_skipped() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let pixels = [1, 2, 3, 4, 5, 6, 99, 99, 7, 8, 9, 10, 11, 12, 88, 88];
    let image = RawThumbnailImage::new(2, 2, 8, RawThumbnailPixelFormat::Rgb8, &pixels).unwrap();

    let installed = root
        .install_thumbnail_raw_png_bytes(&original, ThumbnailSize::Normal, image)
        .unwrap();

    let (width, height, rgba) = decode_rgba(installed.png_bytes());
    assert_eq!((width, height), (2, 2));
    assert_eq!(
        rgba,
        [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
    );
}

#[test]
fn raw_thumbnail_oversized_input_is_downscaled() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let width = 300;
    let height = 150;
    let stride = width * 3;
    let pixels = vec![128; stride as usize * height as usize];
    let image = RawThumbnailImage::new(
        width,
        height,
        stride as usize,
        RawThumbnailPixelFormat::Rgb8,
        &pixels,
    )
    .unwrap();

    let installed = root
        .install_thumbnail_raw_png_bytes(&original, ThumbnailSize::Normal, image)
        .unwrap();

    let parsed = ParsedThumbnailPng::parse(installed.png_bytes()).unwrap();
    assert_eq!(parsed.width(), 128);
    assert_eq!(parsed.height(), 64);
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
}

#[test]
fn raw_thumbnail_rejects_invalid_stride() {
    let pixels = [0; 6];

    let error =
        RawThumbnailImage::new(2, 1, 5, RawThumbnailPixelFormat::Rgb8, &pixels).unwrap_err();

    assert!(matches!(
        error,
        ThumbnailError::UnsupportedRenderedThumbnail("raw thumbnail stride is too small")
    ));
}

#[test]
fn raw_thumbnail_rejects_short_buffer() {
    let pixels = [0; 13];

    let error =
        RawThumbnailImage::new(2, 2, 8, RawThumbnailPixelFormat::Rgb8, &pixels).unwrap_err();

    assert!(matches!(
        error,
        ThumbnailError::UnsupportedRenderedThumbnail("raw thumbnail buffer is too short")
    ));
}

fn readable_original() -> ReadableOriginalIdentity {
    ReadableOriginalIdentity::from_confirmed_readable_identity(
        OriginalIdentity::with_mime_type(
            PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            UnixMTimeSeconds::new(42),
            Some(12),
            "image/png",
        )
        .unwrap(),
    )
}

fn png_without_metadata(width: u32, height: u32, color_type: png::ColorType) -> Vec<u8> {
    let channels = match color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => unimplemented!("test helper only supports RGB/RGBA"),
    };
    let pixels = vec![255; width as usize * height as usize * channels];
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
    }
    output
}

fn decode_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut output = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut output).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    output.truncate(info.buffer_size());
    (info.width, info.height, output)
}
