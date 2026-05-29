// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, SharedCacheEntryOutcome, SharedRepositoryContext, SharedThumbnailLookup,
    ThumbnailSize, UnixMTimeSeconds,
};

#[test]
fn shared_lookup_distinguishes_missing_verified_incomplete_invalid_and_unverifiable() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    assert_eq!(
        context
            .validated_thumbnail_path(
                Some(UnixMTimeSeconds::new(42)),
                Some(12),
                ThumbnailSize::Normal
            )
            .unwrap(),
        SharedThumbnailLookup::Missing
    );

    let path = context.thumbnail_path(ThumbnailSize::Normal);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let verified = shared_png(metadata("./picture.png", Some("42"), Some("12")));
    std::fs::write(&path, &verified).unwrap();
    match context
        .validated_thumbnail_payload(
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
            ThumbnailSize::Normal,
        )
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(payload) => {
            assert_eq!(payload.path(), path.as_path());
            assert_eq!(payload.bytes(), verified.as_slice());
        }
        other => panic!("expected fully verified shared payload, got {other:?}"),
    }

    std::fs::write(&path, shared_png(BTreeMap::new())).unwrap();
    match context
        .validated_thumbnail_path(
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
            ThumbnailSize::Normal,
        )
        .unwrap()
    {
        SharedThumbnailLookup::MetadataIncomplete(entry) => {
            assert_eq!(entry.path(), path.as_path());
        }
        other => panic!("expected metadata-incomplete shared path, got {other:?}"),
    }

    std::fs::write(
        &path,
        shared_png(metadata("./other.png", Some("42"), Some("12"))),
    )
    .unwrap();
    match context
        .validated_thumbnail_path(
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
            ThumbnailSize::Normal,
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
        .validated_thumbnail_path(None, Some(12), ThumbnailSize::Normal)
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
