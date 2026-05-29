// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, CacheRoot, OriginalIdentity, ParsedThumbnailPng, PersonalThumbnailUri,
    ReadableOriginalIdentity, ThumbnailSize, UnixMTimeSeconds,
};

#[test]
fn installs_normalized_downscaled_personal_thumbnail_atomically() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let rendered = png_without_metadata(300, 150, png::ColorType::Rgb);

    let installed = root
        .install_personal_thumbnail_payload(&original, ThumbnailSize::Normal, &rendered)
        .unwrap();

    let expected_path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    assert_eq!(installed.path(), expected_path.as_path());
    assert_eq!(std::fs::read(&expected_path).unwrap(), installed.bytes());

    let parsed = ParsedThumbnailPng::parse(installed.bytes()).unwrap();
    assert_eq!(parsed.width(), 128);
    assert_eq!(parsed.height(), 64);
    assert_eq!(parsed.bit_depth(), png::BitDepth::Eight);
    assert_eq!(parsed.color_type(), png::ColorType::Rgba);
    assert!(!parsed.interlaced());
    assert_eq!(
        parsed.metadata().thumb_uri(),
        Some("file:///home/alice/photo.png")
    );
    assert_eq!(parsed.metadata().thumb_mtime(), Some(42));
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
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let rendered = png_without_metadata(2, 1, png::ColorType::Rgba);

    let installed = root
        .install_personal_thumbnail_path(&original, ThumbnailSize::Normal, &rendered)
        .unwrap();

    let expected_path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    assert_eq!(installed.path(), expected_path.as_path());
    assert!(expected_path.exists());
}

#[test]
fn install_rejects_insecure_existing_cache_directories() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let target_dir = root.as_path().join("normal");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::set_permissions(&target_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let error = root
        .install_personal_thumbnail_payload(
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
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(root.as_path()).unwrap();
    symlink(&outside, root.as_path().join("normal")).unwrap();

    let error = root
        .install_personal_thumbnail_payload(
            &readable_original(),
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1, png::ColorType::Rgba),
        )
        .unwrap_err();

    assert!(error.to_string().contains("insecure"));
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
}

fn readable_original() -> ReadableOriginalIdentity {
    ReadableOriginalIdentity::new(
        OriginalIdentity::with_mime_type(
            PersonalThumbnailUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
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
