// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, SharedCacheEntryOutcome, SharedOriginalFacts, SharedOriginalMetadata,
    SharedRepositoryContext, SharedThumbnailLookup, SharedThumbnailMetadataPolicy,
    ThumbnailMetadataKey, ThumbnailMetadataProblem, ThumbnailMetadataProblemKind, ThumbnailSize,
    UnixMtimeSeconds,
};

#[test]
fn shared_original_metadata_builder_feeds_lookup_facts() {
    let metadata = SharedOriginalMetadata::new()
        .with_mtime(UnixMtimeSeconds::new(42))
        .with_original_byte_size(12);
    let facts = SharedOriginalFacts::new(SharedThumbnailMetadataPolicy::RequireComplete, metadata);

    assert_eq!(
        facts.metadata_policy(),
        SharedThumbnailMetadataPolicy::RequireComplete
    );
    assert_eq!(facts.mtime(), Some(UnixMtimeSeconds::new(42)));
    assert_eq!(facts.original_byte_size(), Some(12));
    assert_eq!(facts.metadata(), metadata);
}

#[test]
fn shared_lookup_distinguishes_missing_verified_incomplete_invalid_and_unverifiable() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    assert_eq!(
        context
            .lookup_thumbnail_path(require_complete_facts(), ThumbnailSize::Normal)
            .unwrap(),
        SharedThumbnailLookup::Missing
    );

    let path = context.cache_entry_path(ThumbnailSize::Normal);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let verified = shared_png(metadata("./picture.png", Some("42"), Some("12")));
    std::fs::write(&path, &verified).unwrap();
    match context
        .lookup_thumbnail_png_bytes(require_complete_facts(), ThumbnailSize::Normal)
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(bytes) => {
            assert_eq!(bytes.path(), path.as_path());
            assert_eq!(bytes.png_bytes(), verified.as_slice());
        }
        other => panic!("expected fully verified shared bytes, got {other:?}"),
    }
    match context
        .lookup_thumbnail_rgba8(require_complete_facts(), ThumbnailSize::Normal)
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(rgba8) => {
            assert_eq!(rgba8.path(), path.as_path());
            assert_eq!(rgba8.width(), 2);
            assert_eq!(rgba8.height(), 1);
            assert_eq!(rgba8.stride(), 8);
            assert_eq!(rgba8.pixels(), &[255; 8]);
        }
        other => panic!("expected fully verified shared RGBA8, got {other:?}"),
    }
    match context
        .lookup_thumbnail_path(allow_incomplete_facts(), ThumbnailSize::Normal)
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(entry) => {
            assert_eq!(entry.path(), path.as_path());
        }
        other => panic!("expected fully verified shared path, got {other:?}"),
    }

    std::fs::write(&path, shared_png(BTreeMap::new())).unwrap();
    match context
        .lookup_thumbnail_path(allow_incomplete_facts(), ThumbnailSize::Normal)
        .unwrap()
    {
        SharedThumbnailLookup::MetadataIncomplete(entry) => {
            assert_eq!(entry.path(), path.as_path());
        }
        other => panic!("expected metadata-incomplete shared path, got {other:?}"),
    }
    match context
        .lookup_thumbnail_rgba8(allow_incomplete_facts(), ThumbnailSize::Normal)
        .unwrap()
    {
        SharedThumbnailLookup::MetadataIncomplete(entry) => {
            assert_eq!(entry.path(), path.as_path());
            assert_eq!(entry.pixels(), &[255; 8]);
            assert_eq!(entry.metadata().thumb_uri(), None);
        }
        other => panic!("expected metadata-incomplete shared RGBA8, got {other:?}"),
    }
    match context
        .lookup_thumbnail_path(require_complete_facts(), ThumbnailSize::Normal)
        .unwrap()
    {
        SharedThumbnailLookup::Invalid(problems) => {
            assert_eq!(
                problems,
                vec![
                    metadata_problem(
                        ThumbnailMetadataKey::Uri,
                        ThumbnailMetadataProblemKind::MissingRequired,
                    ),
                    metadata_problem(
                        ThumbnailMetadataKey::Mtime,
                        ThumbnailMetadataProblemKind::MissingRequired,
                    ),
                ]
            );
        }
        other => panic!("expected incomplete shared path rejection, got {other:?}"),
    }

    std::fs::write(
        &path,
        shared_png(metadata("./other.png", Some("42"), Some("12"))),
    )
    .unwrap();
    match context
        .lookup_thumbnail_path(require_complete_facts(), ThumbnailSize::Normal)
        .unwrap()
    {
        SharedThumbnailLookup::Invalid(problems) => {
            assert!(problems.contains(&metadata_problem(
                ThumbnailMetadataKey::Uri,
                ThumbnailMetadataProblemKind::ValueMismatch,
            )));
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
            SharedOriginalFacts::new(
                SharedThumbnailMetadataPolicy::AllowIncomplete,
                SharedOriginalMetadata::new().with_original_byte_size(12),
            ),
            ThumbnailSize::Normal,
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
    let path = context.cache_entry_path(ThumbnailSize::Normal);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        shared_png(metadata("./picture.png", Some("42"), Some("12"))),
    )
    .unwrap();

    let inspections = context
        .inspect_thumbnails(&[ThumbnailSize::Normal], original_metadata())
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
    let path = context.cache_entry_path(ThumbnailSize::Normal);
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
            .lookup_thumbnail_png_bytes(require_complete_facts(), ThumbnailSize::Normal)
            .unwrap(),
    );

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_unreadable_shared_lookup(
        context
            .lookup_thumbnail_path(require_complete_facts(), ThumbnailSize::Normal)
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

fn metadata_problem(
    key: ThumbnailMetadataKey,
    kind: ThumbnailMetadataProblemKind,
) -> CacheEntryProblem {
    CacheEntryProblem::Metadata(ThumbnailMetadataProblem::new(key, kind))
}

fn require_complete_facts() -> SharedOriginalFacts {
    SharedOriginalFacts::new(
        SharedThumbnailMetadataPolicy::RequireComplete,
        original_metadata(),
    )
}

fn allow_incomplete_facts() -> SharedOriginalFacts {
    SharedOriginalFacts::new(
        SharedThumbnailMetadataPolicy::AllowIncomplete,
        original_metadata(),
    )
}

fn original_metadata() -> SharedOriginalMetadata {
    SharedOriginalMetadata::new()
        .with_mtime(UnixMtimeSeconds::new(42))
        .with_original_byte_size(12)
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
