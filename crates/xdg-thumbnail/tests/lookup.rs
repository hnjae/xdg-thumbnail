// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, CacheNamespace, CacheRoot, OriginalIdentity, PersonalThumbnailUri,
    ReadableOriginalIdentity, ThumbnailLookup, ThumbnailSize, UnixMTimeSeconds,
};

#[test]
fn validated_path_lookup_distinguishes_valid_missing_and_invalid_entries() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    assert_eq!(
        root.validated_personal_path(&original, ThumbnailSize::Normal)
            .unwrap(),
        ThumbnailLookup::Missing
    );

    let valid_bytes = png_with_metadata(metadata("42"));
    std::fs::write(&path, &valid_bytes).unwrap();
    match root
        .validated_personal_path(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        ThumbnailLookup::Valid(valid) => {
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
        .validated_personal_path(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        ThumbnailLookup::Invalid(problems) => {
            assert!(problems.contains(&CacheEntryProblem::StaleMetadata));
        }
        other => panic!("expected invalid lookup, got {other:?}"),
    }
}

#[test]
fn validated_payload_lookup_returns_exact_validated_png_bytes() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let valid_bytes = png_with_metadata(metadata("42"));
    std::fs::write(&path, &valid_bytes).unwrap();

    match root
        .validated_personal_payload(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        ThumbnailLookup::Valid(valid) => {
            assert_eq!(valid.path(), path.as_path());
            assert_eq!(valid.bytes(), valid_bytes.as_slice());
            assert_eq!(valid.metadata().thumb_size(), Some(12));
        }
        other => panic!("expected valid payload lookup, got {other:?}"),
    }
}

fn original_identity(mtime: i64) -> ReadableOriginalIdentity {
    ReadableOriginalIdentity::new(
        OriginalIdentity::with_mime_type(
            PersonalThumbnailUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            UnixMTimeSeconds::new(mtime),
            Some(12),
            "image/png",
        )
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
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        for (key, value) in metadata {
            encoder
                .add_text_chunk(key.to_owned(), value.to_owned())
                .unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 8]).unwrap();
    }
    output
}
