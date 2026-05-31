// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, CacheNamespace, PersonalCacheRoot, PersonalOriginalIdentity,
    PersonalOriginalUri, PersonalThumbnailLookup, ReadablePersonalOriginalIdentity,
    ThumbnailMetadataKey, ThumbnailMetadataProblem, ThumbnailMetadataProblemKind, ThumbnailSize,
    UnixMtimeSeconds,
};

#[test]
fn lookup_thumbnail_path_distinguishes_valid_missing_and_invalid_entries() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    assert_eq!(
        root.lookup_thumbnail_path(&original, ThumbnailSize::Normal)
            .unwrap(),
        PersonalThumbnailLookup::Missing
    );

    let valid_bytes = png_with_metadata(metadata("42"));
    std::fs::write(&path, &valid_bytes).unwrap();
    match root
        .lookup_thumbnail_path(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(valid) => {
            assert_eq!(valid.path(), path.as_path());
            assert_eq!(
                valid.metadata().thumb_uri(),
                Some("file:///home/alice/photo.png")
            );
        }
        other => panic!("expected valid path lookup, got {other:?}"),
    }

    std::fs::write(&path, png_with_metadata(metadata("41"))).unwrap();
    match root
        .lookup_thumbnail_path(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Invalid(problems) => {
            assert!(problems.contains(&metadata_problem(
                ThumbnailMetadataKey::Mtime,
                ThumbnailMetadataProblemKind::ValueMismatch,
            )));
        }
        other => panic!("expected invalid lookup, got {other:?}"),
    }
}

#[test]
fn lookup_thumbnail_png_bytes_returns_exact_validated_png_bytes() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let valid_bytes = png_with_metadata(metadata("42"));
    std::fs::write(&path, &valid_bytes).unwrap();

    match root
        .lookup_thumbnail_png_bytes(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(valid) => {
            assert_eq!(valid.path(), path.as_path());
            assert_eq!(valid.png_bytes(), valid_bytes.as_slice());
            assert_eq!(valid.metadata().thumb_size(), Some(12));
        }
        other => panic!("expected valid bytes lookup, got {other:?}"),
    }
}

#[test]
fn lookup_thumbnail_rgba8_returns_decoded_pixels_and_metadata() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let pixels = [1, 2, 3, 4, 5, 6, 7, 8];
    std::fs::write(
        &path,
        png_with_metadata_pixels(metadata("42"), png::ColorType::Rgba, &pixels),
    )
    .unwrap();

    match root
        .lookup_thumbnail_rgba8(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(valid) => {
            assert_eq!(valid.path(), path.as_path());
            assert_eq!(valid.width(), 2);
            assert_eq!(valid.height(), 1);
            assert_eq!(valid.stride(), 8);
            assert_eq!(valid.pixels(), pixels.as_slice());
            assert_eq!(valid.metadata().thumb_size(), Some(12));

            let parts = valid.into_parts();
            assert_eq!(parts.path, path);
            assert_eq!((parts.width, parts.height, parts.stride), (2, 1, 8));
            assert_eq!(parts.pixels, pixels);
            assert_eq!(
                parts.metadata.thumb_mtime(),
                Some(UnixMtimeSeconds::new(42))
            );
        }
        other => panic!("expected valid RGBA8 lookup, got {other:?}"),
    }
}

#[test]
fn lookup_thumbnail_rgba8_converts_grayscale_alpha_pixels() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        png_with_metadata_pixels(
            metadata("42"),
            png::ColorType::GrayscaleAlpha,
            &[10, 20, 30, 40],
        ),
    )
    .unwrap();

    match root
        .lookup_thumbnail_rgba8(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(valid) => {
            assert_eq!(valid.width(), 2);
            assert_eq!(valid.height(), 1);
            assert_eq!(valid.stride(), 8);
            assert_eq!(valid.pixels(), &[10, 10, 10, 20, 30, 30, 30, 40]);
        }
        other => panic!("expected valid grayscale-alpha RGBA8 lookup, got {other:?}"),
    }
}

#[test]
fn lookup_thumbnail_rgba8_keeps_rgb_without_alpha_invalid() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        png_with_metadata_pixels(metadata("42"), png::ColorType::Rgb, &[1, 2, 3, 4, 5, 6]),
    )
    .unwrap();

    match root
        .lookup_thumbnail_rgba8(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Invalid(problems) => {
            assert_eq!(problems, vec![CacheEntryProblem::NonconformingPngFormat]);
        }
        other => panic!("expected nonconforming RGBA8 lookup, got {other:?}"),
    }
}

#[test]
fn validated_lookup_rejects_symlink_and_non_regular_entries() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let outside = temp.path().join("outside.png");
    std::fs::write(&outside, png_with_metadata(metadata("42"))).unwrap();
    symlink(&outside, &path).unwrap();

    assert_unreadable_lookup(
        root.lookup_thumbnail_png_bytes(&original, ThumbnailSize::Normal)
            .unwrap(),
    );

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_unreadable_lookup(
        root.lookup_thumbnail_path(&original, ThumbnailSize::Normal)
            .unwrap(),
    );
}

fn assert_unreadable_lookup<T: std::fmt::Debug>(lookup: PersonalThumbnailLookup<T>) {
    match lookup {
        PersonalThumbnailLookup::Invalid(problems) => {
            assert_eq!(problems, vec![CacheEntryProblem::UnreadableEntry]);
        }
        other => panic!("expected unreadable invalid lookup, got {other:?}"),
    }
}

fn metadata_problem(
    key: ThumbnailMetadataKey,
    kind: ThumbnailMetadataProblemKind,
) -> CacheEntryProblem {
    CacheEntryProblem::Metadata(ThumbnailMetadataProblem::new(key, kind))
}

fn original_identity(mtime: u64) -> ReadablePersonalOriginalIdentity {
    ReadablePersonalOriginalIdentity::from_confirmed_readable_identity(
        PersonalOriginalIdentity::new(
            PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            UnixMtimeSeconds::new(mtime),
        )
        .with_original_byte_size(12)
        .with_mime_type("image/png")
        .unwrap(),
    )
}

fn metadata(mtime: &'static str) -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("Thumb::URI", "file:///home/alice/photo.png"),
        ("Thumb::MTime", mtime),
        ("Thumb::Size", "12"),
        ("Thumb::Mimetype", "image/png"),
    ])
}

fn png_with_metadata(metadata: BTreeMap<&str, &str>) -> Vec<u8> {
    png_with_metadata_pixels(metadata, png::ColorType::Rgba, &[255; 8])
}

fn png_with_metadata_pixels(
    metadata: BTreeMap<&str, &str>,
    color_type: png::ColorType,
    pixels: &[u8],
) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, 2, 1);
        encoder.set_color(color_type);
        encoder.set_depth(png::BitDepth::Eight);
        for (key, value) in metadata {
            encoder
                .add_text_chunk(key.to_owned(), value.to_owned())
                .unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
    }
    output
}
