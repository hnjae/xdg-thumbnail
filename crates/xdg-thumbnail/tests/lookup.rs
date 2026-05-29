// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, CacheNamespace, OriginalIdentity, PersonalCacheRoot, PersonalOriginalUri,
    PersonalThumbnailLookup, ReadableOriginalIdentity, ThumbnailSize, UnixMTimeSeconds,
};

#[test]
fn validated_path_lookup_distinguishes_valid_missing_and_invalid_entries() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    assert_eq!(
        root.validated_personal_path(&original, ThumbnailSize::Normal)
            .unwrap(),
        PersonalThumbnailLookup::Missing
    );

    let valid_bytes = png_with_metadata(metadata("42"));
    std::fs::write(&path, &valid_bytes).unwrap();
    match root
        .validated_personal_path(&original, ThumbnailSize::Normal)
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
        .validated_personal_path(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Invalid(problems) => {
            assert!(problems.contains(&CacheEntryProblem::StaleMetadata));
        }
        other => panic!("expected invalid lookup, got {other:?}"),
    }
}

#[test]
fn validated_bytes_lookup_returns_exact_validated_png_bytes() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let valid_bytes = png_with_metadata(metadata("42"));
    std::fs::write(&path, &valid_bytes).unwrap();

    match root
        .validated_personal_bytes(&original, ThumbnailSize::Normal)
        .unwrap()
    {
        PersonalThumbnailLookup::Valid(valid) => {
            assert_eq!(valid.path(), path.as_path());
            assert_eq!(valid.bytes(), valid_bytes.as_slice());
            assert_eq!(valid.metadata().thumb_size(), Some(12));
        }
        other => panic!("expected valid bytes lookup, got {other:?}"),
    }
}

#[test]
fn validated_lookup_rejects_symlink_and_non_regular_entries() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = original_identity(42);
    let path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let outside = temp.path().join("outside.png");
    std::fs::write(&outside, png_with_metadata(metadata("42"))).unwrap();
    symlink(&outside, &path).unwrap();

    assert_unreadable_lookup(
        root.validated_personal_bytes(&original, ThumbnailSize::Normal)
            .unwrap(),
    );

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_unreadable_lookup(
        root.validated_personal_path(&original, ThumbnailSize::Normal)
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

fn original_identity(mtime: u64) -> ReadableOriginalIdentity {
    ReadableOriginalIdentity::from_confirmed_readable_identity(
        OriginalIdentity::with_mime_type(
            PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
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
