// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, SharedCacheEntryOutcome, SharedRepositoryContext, SharedThumbnailLookup,
    SharedThumbnailMetadataPolicy, ThumbnailSize, UnixMTimeSeconds,
};

#[test]
fn shared_lookup_distinguishes_missing_verified_incomplete_invalid_and_unverifiable() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    assert_eq!(
        context
            .lookup_thumbnail_path(
                ThumbnailSize::Normal,
                SharedThumbnailMetadataPolicy::RequireComplete,
                Some(UnixMTimeSeconds::new(42)),
                Some(12),
            )
            .unwrap(),
        SharedThumbnailLookup::Missing
    );

    let path = context.thumbnail_path(ThumbnailSize::Normal);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let verified = shared_png(metadata("./picture.png", Some("42"), Some("12")));
    std::fs::write(&path, &verified).unwrap();
    match context
        .lookup_thumbnail_png_bytes(
            ThumbnailSize::Normal,
            SharedThumbnailMetadataPolicy::RequireComplete,
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
        )
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(bytes) => {
            assert_eq!(bytes.path(), path.as_path());
            assert_eq!(bytes.png_bytes(), verified.as_slice());
        }
        other => panic!("expected fully verified shared bytes, got {other:?}"),
    }
    match context
        .lookup_thumbnail_path(
            ThumbnailSize::Normal,
            SharedThumbnailMetadataPolicy::AllowIncomplete,
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
        )
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(entry) => {
            assert_eq!(entry.path(), path.as_path());
        }
        other => panic!("expected fully verified shared path, got {other:?}"),
    }

    std::fs::write(&path, shared_png(BTreeMap::new())).unwrap();
    match context
        .lookup_thumbnail_path(
            ThumbnailSize::Normal,
            SharedThumbnailMetadataPolicy::AllowIncomplete,
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
        )
        .unwrap()
    {
        SharedThumbnailLookup::MetadataIncomplete(entry) => {
            assert_eq!(entry.path(), path.as_path());
        }
        other => panic!("expected metadata-incomplete shared path, got {other:?}"),
    }
    match context
        .lookup_thumbnail_path(
            ThumbnailSize::Normal,
            SharedThumbnailMetadataPolicy::RequireComplete,
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
        )
        .unwrap()
    {
        SharedThumbnailLookup::Invalid(problems) => {
            assert_eq!(problems, vec![CacheEntryProblem::MissingRequiredMetadata]);
        }
        other => panic!("expected incomplete shared path rejection, got {other:?}"),
    }

    std::fs::write(
        &path,
        shared_png(metadata("./other.png", Some("42"), Some("12"))),
    )
    .unwrap();
    match context
        .lookup_thumbnail_path(
            ThumbnailSize::Normal,
            SharedThumbnailMetadataPolicy::RequireComplete,
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
        )
        .unwrap()
    {
        SharedThumbnailLookup::Invalid(problems) => {
            assert!(problems.contains(&CacheEntryProblem::StaleMetadata));
        }
        other => panic!("expected invalid shared path, got {other:?}"),
    }

    std::fs::write(
        &path,
        shared_png(metadata("./picture.png", Some("42"), Some("12"))),
    )
    .unwrap();
    match context
        .lookup_thumbnail_path(
            ThumbnailSize::Normal,
            SharedThumbnailMetadataPolicy::AllowIncomplete,
            None,
            Some(12),
        )
        .unwrap()
    {
        SharedThumbnailLookup::Unverifiable(problems) => {
            assert_eq!(problems, vec![CacheEntryProblem::UnverifiableOriginal]);
        }
        other => panic!("expected unverifiable shared path, got {other:?}"),
    }
}

#[test]
fn shared_inspection_reports_read_only_facts_without_removal_handle() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let path = context.thumbnail_path(ThumbnailSize::Normal);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        shared_png(metadata("./picture.png", Some("42"), Some("12"))),
    )
    .unwrap();

    let inspections = context
        .inspect_thumbnails(
            &[ThumbnailSize::Normal],
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
        )
        .unwrap();

    assert_eq!(inspections.len(), 1);
    assert_eq!(
        inspections[0].outcome(),
        &SharedCacheEntryOutcome::FullyVerified
    );
    assert_eq!(inspections[0].shared_uri().as_str(), "./picture.png");
    assert_eq!(inspections[0].size(), ThumbnailSize::Normal);
    assert_eq!(inspections[0].path(), path.as_path());
    assert_eq!(
        inspections[0]
            .metadata()
            .and_then(|metadata| metadata.thumb_uri()),
        Some("./picture.png")
    );
}

#[test]
fn shared_validated_lookup_rejects_symlink_and_non_regular_entries() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let path = context.thumbnail_path(ThumbnailSize::Normal);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let outside = temp.path().join("outside.png");
    std::fs::write(
        &outside,
        shared_png(metadata("./picture.png", Some("42"), Some("12"))),
    )
    .unwrap();
    symlink(&outside, &path).unwrap();

    assert_unreadable_shared_lookup(
        context
            .lookup_thumbnail_png_bytes(
                ThumbnailSize::Normal,
                SharedThumbnailMetadataPolicy::RequireComplete,
                Some(UnixMTimeSeconds::new(42)),
                Some(12),
            )
            .unwrap(),
    );

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_unreadable_shared_lookup(
        context
            .lookup_thumbnail_path(
                ThumbnailSize::Normal,
                SharedThumbnailMetadataPolicy::RequireComplete,
                Some(UnixMTimeSeconds::new(42)),
                Some(12),
            )
            .unwrap(),
    );
}

fn assert_unreadable_shared_lookup<T: std::fmt::Debug>(lookup: SharedThumbnailLookup<T>) {
    match lookup {
        SharedThumbnailLookup::Invalid(problems) => {
            assert_eq!(problems, vec![CacheEntryProblem::UnreadableEntry]);
        }
        other => panic!("expected unreadable invalid shared lookup, got {other:?}"),
    }
}

fn metadata(
    uri: &'static str,
    mtime: Option<&'static str>,
    size: Option<&'static str>,
) -> BTreeMap<&'static str, &'static str> {
    let mut metadata = BTreeMap::from([("Thumb::URI", uri)]);
    if let Some(mtime) = mtime {
        metadata.insert("Thumb::MTime", mtime);
    }
    if let Some(size) = size {
        metadata.insert("Thumb::Size", size);
    }
    metadata
}

fn shared_png(metadata: BTreeMap<&str, &str>) -> Vec<u8> {
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
