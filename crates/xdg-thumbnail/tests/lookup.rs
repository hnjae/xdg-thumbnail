// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, CacheNamespace, PersonalCacheRoot, PersonalOriginalIdentity,
    PersonalOriginalUri, PersonalThumbnailLookup, ReadablePersonalOriginalIdentity,
    ThumbnailMetadataKey, ThumbnailMetadataProblem, ThumbnailMetadataProblemKind,
    ThumbnailPngColorType, ThumbnailSize, UnixMtimeSeconds,
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
            assert_eq!(valid.metadata().thumb_size_lossy(), Some(12));
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
            assert_eq!(valid.metadata().thumb_size_lossy(), Some(12));

            let parts = valid.into_parts();
            assert_eq!(parts.path, path);
            assert_eq!((parts.width, parts.height, parts.stride), (2, 1, 8));
            assert_eq!(parts.pixels, pixels);
            assert_eq!(
                parts.metadata.thumb_mtime_lossy(),
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
fn personal_thumbnail_lookup_matches_exhaustively() {
    let lookup: PersonalThumbnailLookup<()> = PersonalThumbnailLookup::Missing;

    let outcome = match lookup {
        PersonalThumbnailLookup::Valid(()) => "valid",
        PersonalThumbnailLookup::Missing => "missing",
        PersonalThumbnailLookup::Invalid(problems) => {
            assert!(problems.is_empty());
            "invalid"
        }
    };

    assert_eq!(outcome, "missing");
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

#[test]
fn display_lookup_uses_larger_personal_source_when_exact_is_missing() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let source_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::XxLarge),
    );
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        png_with_metadata_dimensions(metadata("42"), 1024, 512, 7),
    )
    .unwrap();

    match root
        .lookup_display_thumbnail_rgba8(&original, ThumbnailSize::XLarge)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(display) => {
            assert_eq!(display.source_path(), source_path.as_path());
            assert_eq!(display.requested_size(), ThumbnailSize::XLarge);
            assert_eq!(display.source_size(), ThumbnailSize::XxLarge);
            assert!(display.is_derived());
            assert_eq!(
                (display.width(), display.height(), display.stride()),
                (512, 256, 2048)
            );
            assert_eq!(display.pixels().len(), 512 * 256 * 4);
            assert_eq!(display.source_metadata().thumb_size_lossy(), Some(12));

            let parts = display.into_parts();
            assert_eq!(parts.source_path, source_path);
            assert_eq!(parts.requested_size, ThumbnailSize::XLarge);
            assert_eq!(parts.source_size, ThumbnailSize::XxLarge);
            assert_eq!(parts.width, 512);
            assert_eq!(parts.height, 256);
            assert_eq!(parts.stride, 2048);
            assert_eq!(
                parts.source_metadata.thumb_mtime_lossy(),
                Some(UnixMtimeSeconds::new(42))
            );
        }
        other => panic!("expected derived personal display lookup, got {other:?}"),
    }
}

#[test]
fn display_lookup_exact_valid_wins_over_larger_personal_source() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let exact_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Large),
    );
    let larger_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::XxLarge),
    );
    std::fs::create_dir_all(exact_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(larger_path.parent().unwrap()).unwrap();
    std::fs::write(
        &exact_path,
        png_with_metadata_dimensions(metadata("42"), 64, 32, 11),
    )
    .unwrap();
    std::fs::write(
        &larger_path,
        png_with_metadata_dimensions(metadata("42"), 1024, 512, 22),
    )
    .unwrap();

    match root
        .lookup_display_thumbnail_rgba8(&original, ThumbnailSize::Large)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(display) => {
            assert_eq!(display.source_path(), exact_path.as_path());
            assert_eq!(display.source_size(), ThumbnailSize::Large);
            assert!(!display.is_derived());
            assert_eq!((display.width(), display.height()), (64, 32));
            assert_eq!(display.pixels()[0], 11);
        }
        other => panic!("expected exact personal display lookup, got {other:?}"),
    }
}

#[test]
fn display_lookup_returns_exact_invalid_without_trying_larger_personal_source() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let exact_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Large),
    );
    let larger_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::XxLarge),
    );
    std::fs::create_dir_all(exact_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(larger_path.parent().unwrap()).unwrap();
    std::fs::write(&exact_path, png_with_metadata(metadata("41"))).unwrap();
    std::fs::write(
        &larger_path,
        png_with_metadata_dimensions(metadata("42"), 1024, 512, 22),
    )
    .unwrap();

    match root
        .lookup_display_thumbnail_rgba8(&original, ThumbnailSize::Large)
        .unwrap()
    {
        PersonalThumbnailLookup::Invalid(problems) => {
            assert!(problems.contains(&metadata_problem(
                ThumbnailMetadataKey::Mtime,
                ThumbnailMetadataProblemKind::ValueMismatch,
            )));
        }
        other => panic!("expected exact invalid display lookup, got {other:?}"),
    }
}

#[test]
fn display_lookup_returns_first_larger_invalid_personal_source() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let first_larger_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Large),
    );
    let second_larger_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::XxLarge),
    );
    std::fs::create_dir_all(first_larger_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(second_larger_path.parent().unwrap()).unwrap();
    std::fs::write(&first_larger_path, png_with_metadata(metadata("41"))).unwrap();
    std::fs::write(
        &second_larger_path,
        png_with_metadata_dimensions(metadata("42"), 1024, 512, 22),
    )
    .unwrap();

    match root
        .lookup_display_thumbnail_rgba8(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Invalid(problems) => {
            assert!(problems.contains(&metadata_problem(
                ThumbnailMetadataKey::Mtime,
                ThumbnailMetadataProblemKind::ValueMismatch,
            )));
        }
        other => panic!("expected first larger invalid display lookup, got {other:?}"),
    }
}

#[test]
fn display_lookup_returns_missing_when_no_personal_candidates_exist() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);

    assert_eq!(
        root.lookup_display_thumbnail_rgba8(&original, ThumbnailSize::Large)
            .unwrap(),
        PersonalThumbnailLookup::Missing
    );
}

#[test]
fn materialize_personal_larger_source_writes_requested_namespace() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let source_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Large),
    );
    let target_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        png_with_metadata_dimensions(metadata("42"), 256, 128, 99),
    )
    .unwrap();

    match root
        .materialize_thumbnail_from_larger_cache_returning_path(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(materialized) => {
            assert_eq!(materialized.target_path(), target_path.as_path());
            assert_eq!(materialized.source_path(), source_path.as_path());
            assert_eq!(materialized.requested_size(), ThumbnailSize::Normal);
            assert_eq!(materialized.source_size(), ThumbnailSize::Large);
            assert!(materialized.written());
            let parts = materialized.into_parts();
            assert_eq!(parts.target_path, target_path);
            assert_eq!(parts.source_path, source_path);
            assert!(parts.written);
        }
        other => panic!("expected personal materialized path, got {other:?}"),
    }

    let parsed =
        xdg_thumbnail::ParsedThumbnailPng::parse(&std::fs::read(&target_path).unwrap()).unwrap();
    assert_eq!((parsed.width(), parsed.height()), (128, 64));
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
    assert_eq!(
        parsed.metadata().thumb_uri(),
        Some("file:///home/alice/photo.png")
    );
    assert_eq!(
        parsed.metadata().thumb_mtime_lossy(),
        Some(UnixMtimeSeconds::new(42))
    );
    assert_eq!(parsed.metadata().thumb_size_lossy(), Some(12));
}

#[test]
fn materialize_personal_exact_valid_is_noop() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let target_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    let exact_bytes = png_with_metadata_dimensions(metadata("42"), 64, 32, 77);
    std::fs::write(&target_path, &exact_bytes).unwrap();

    match root
        .materialize_thumbnail_from_larger_cache_returning_path(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(materialized) => {
            assert_eq!(materialized.target_path(), target_path.as_path());
            assert_eq!(materialized.source_path(), target_path.as_path());
            assert_eq!(materialized.source_size(), ThumbnailSize::Normal);
            assert!(!materialized.written());
        }
        other => panic!("expected no-op personal materialization, got {other:?}"),
    }
    assert_eq!(std::fs::read(&target_path).unwrap(), exact_bytes);
}

#[test]
fn materialize_personal_png_bytes_returns_final_target_bytes() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let source_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Large),
    );
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        png_with_metadata_dimensions(metadata("42"), 256, 128, 99),
    )
    .unwrap();

    match root
        .materialize_thumbnail_from_larger_cache_returning_png_bytes(
            &original,
            ThumbnailSize::Normal,
        )
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(materialized) => {
            assert_ne!(
                materialized.png_bytes(),
                std::fs::read(&source_path).unwrap().as_slice()
            );
            assert_eq!(
                materialized.png_bytes(),
                std::fs::read(materialized.target_path())
                    .unwrap()
                    .as_slice()
            );
            let parts = materialized.into_parts();
            assert_eq!(parts.png_bytes, std::fs::read(parts.target_path).unwrap());
        }
        other => panic!("expected materialized PNG bytes, got {other:?}"),
    }
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
    ReadablePersonalOriginalIdentity::assume_readable(
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

fn png_with_metadata_dimensions(
    metadata: BTreeMap<&str, &str>,
    width: u32,
    height: u32,
    value: u8,
) -> Vec<u8> {
    let pixels = vec![value; width as usize * height as usize * 4];
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        for (key, value) in metadata {
            encoder
                .add_text_chunk(key.to_owned(), value.to_owned())
                .unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
    }
    output
}
