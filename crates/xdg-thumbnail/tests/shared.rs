// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, CacheNamespace, PersonalCacheRoot, PersonalOriginalUri,
    SharedCacheEntryOutcome, SharedOriginalFacts, SharedOriginalMetadata, SharedRepositoryContext,
    SharedThumbnailLookup, SharedThumbnailMetadataPolicy, ThumbnailError, ThumbnailMetadataKey,
    ThumbnailMetadataProblem, ThumbnailMetadataProblemKind, ThumbnailPngColorType, ThumbnailSize,
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

#[test]
fn shared_display_lookup_uses_fully_verified_larger_source() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let source_path = context.cache_entry_path(ThumbnailSize::XxLarge);
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        shared_png_dimensions(
            metadata("./picture.png", Some("42"), Some("12")),
            1024,
            512,
            9,
        ),
    )
    .unwrap();

    match context
        .lookup_display_thumbnail_rgba8(require_complete_facts(), ThumbnailSize::XLarge)
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(display) => {
            assert_eq!(display.source_path(), source_path.as_path());
            assert_eq!(display.requested_size(), ThumbnailSize::XLarge);
            assert_eq!(display.source_size(), ThumbnailSize::XxLarge);
            assert!(display.is_derived());
            assert_eq!(
                (display.width(), display.height(), display.stride()),
                (512, 256, 2048)
            );
            assert_eq!(display.pixels().len(), 512 * 256 * 4);
            assert_eq!(display.source_metadata().thumb_uri(), Some("./picture.png"));
        }
        other => panic!("expected fully verified shared display lookup, got {other:?}"),
    }
}

#[test]
fn shared_display_lookup_returns_metadata_incomplete_when_policy_allows_it() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let source_path = context.cache_entry_path(ThumbnailSize::Large);
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        shared_png_dimensions(BTreeMap::new(), 256, 128, 9),
    )
    .unwrap();

    match context
        .lookup_display_thumbnail_rgba8(allow_incomplete_facts(), ThumbnailSize::Normal)
        .unwrap()
    {
        SharedThumbnailLookup::MetadataIncomplete(display) => {
            assert_eq!(display.source_path(), source_path.as_path());
            assert_eq!(display.source_size(), ThumbnailSize::Large);
            assert!(display.is_derived());
            assert_eq!((display.width(), display.height()), (128, 64));
            assert_eq!(display.source_metadata().thumb_uri(), None);
        }
        other => panic!("expected metadata-incomplete shared display lookup, got {other:?}"),
    }
}

#[test]
fn shared_display_lookup_propagates_require_complete_invalid_missing_and_unverifiable() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();

    assert_eq!(
        context
            .lookup_display_thumbnail_rgba8(require_complete_facts(), ThumbnailSize::Normal)
            .unwrap(),
        SharedThumbnailLookup::Missing
    );

    let first_larger = context.cache_entry_path(ThumbnailSize::Large);
    let second_larger = context.cache_entry_path(ThumbnailSize::XxLarge);
    std::fs::create_dir_all(first_larger.parent().unwrap()).unwrap();
    std::fs::create_dir_all(second_larger.parent().unwrap()).unwrap();
    std::fs::write(
        &first_larger,
        shared_png_dimensions(BTreeMap::new(), 256, 128, 1),
    )
    .unwrap();
    std::fs::write(
        &second_larger,
        shared_png_dimensions(
            metadata("./picture.png", Some("42"), Some("12")),
            1024,
            512,
            2,
        ),
    )
    .unwrap();

    match context
        .lookup_display_thumbnail_rgba8(require_complete_facts(), ThumbnailSize::Normal)
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
        other => panic!("expected policy-rejected shared display lookup, got {other:?}"),
    }

    std::fs::write(
        &first_larger,
        shared_png_dimensions(
            metadata("./picture.png", Some("42"), Some("12")),
            256,
            128,
            1,
        ),
    )
    .unwrap();
    match context
        .lookup_display_thumbnail_rgba8(
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
        other => panic!("expected unverifiable shared display lookup, got {other:?}"),
    }
}

#[test]
fn materialize_shared_thumbnail_writes_personal_target_metadata() {
    let temp = TempDir::new().unwrap();
    let personal = PersonalCacheRoot::new(temp.path().join("personal-thumbnails")).unwrap();
    let shared =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let source_path = shared.cache_entry_path(ThumbnailSize::Large);
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        shared_png_dimensions(
            metadata("./picture.png", Some("42"), Some("12")),
            256,
            128,
            7,
        ),
    )
    .unwrap();

    match personal
        .materialize_shared_thumbnail_returning_path(
            &shared,
            require_complete_facts(),
            ThumbnailSize::Normal,
        )
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(materialized) => {
            assert_eq!(materialized.source_path(), source_path.as_path());
            assert_eq!(materialized.requested_size(), ThumbnailSize::Normal);
            assert_eq!(materialized.source_size(), ThumbnailSize::Large);
            assert!(materialized.written());
            assert!(materialized.target_path().exists());
        }
        other => panic!("expected shared-to-personal materialized path, got {other:?}"),
    }

    let original_path = temp.path().join("picture.png");
    let expected_uri =
        PersonalOriginalUri::from_absolute_path_bytes(original_path.as_os_str().as_bytes())
            .unwrap();
    let target_path =
        personal.cache_entry_path(&expected_uri, &CacheNamespace::Size(ThumbnailSize::Normal));
    let parsed =
        xdg_thumbnail::ParsedThumbnailPng::parse(&std::fs::read(target_path).unwrap()).unwrap();
    assert_eq!((parsed.width(), parsed.height()), (128, 64));
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
    assert_eq!(parsed.metadata().thumb_uri(), Some(expected_uri.as_str()));
    assert_eq!(
        parsed.metadata().thumb_mtime_lossy(),
        Some(UnixMtimeSeconds::new(42))
    );
    assert_eq!(parsed.metadata().thumb_size_lossy(), Some(12));
}

#[test]
fn materialize_shared_thumbnail_requires_supplied_mtime_and_writes_nothing() {
    let temp = TempDir::new().unwrap();
    let personal = PersonalCacheRoot::new(temp.path().join("personal-thumbnails")).unwrap();
    let shared =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let source_path = shared.cache_entry_path(ThumbnailSize::Large);
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        shared_png_dimensions(metadata("./picture.png", None, Some("12")), 256, 128, 7),
    )
    .unwrap();
    let facts = SharedOriginalFacts::new(
        SharedThumbnailMetadataPolicy::AllowIncomplete,
        SharedOriginalMetadata::new().with_original_byte_size(12),
    );

    let error = personal
        .materialize_shared_thumbnail_returning_path(&shared, facts, ThumbnailSize::Normal)
        .unwrap_err();
    assert!(matches!(error, ThumbnailError::InvalidMetadata { .. }));

    let original_path = temp.path().join("picture.png");
    let expected_uri =
        PersonalOriginalUri::from_absolute_path_bytes(original_path.as_os_str().as_bytes())
            .unwrap();
    let target_path =
        personal.cache_entry_path(&expected_uri, &CacheNamespace::Size(ThumbnailSize::Normal));
    assert!(!target_path.exists());
}

#[test]
fn materialize_shared_thumbnail_png_bytes_are_final_target_bytes() {
    let temp = TempDir::new().unwrap();
    let personal = PersonalCacheRoot::new(temp.path().join("personal-thumbnails")).unwrap();
    let shared =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let source_path = shared.cache_entry_path(ThumbnailSize::Large);
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        shared_png_dimensions(
            metadata("./picture.png", Some("42"), Some("12")),
            256,
            128,
            7,
        ),
    )
    .unwrap();

    match personal
        .materialize_shared_thumbnail_returning_png_bytes(
            &shared,
            require_complete_facts(),
            ThumbnailSize::Normal,
        )
        .unwrap()
    {
        SharedThumbnailLookup::FullyVerified(materialized) => {
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
        }
        other => panic!("expected shared-to-personal PNG bytes, got {other:?}"),
    }
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
    shared_png_dimensions(metadata, 2, 1, 255)
}

fn shared_png_dimensions(
    metadata: BTreeMap<&str, &str>,
    width: u32,
    height: u32,
    value: u8,
) -> Vec<u8> {
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
        writer
            .write_image_data(&vec![value; width as usize * height as usize * 4])
            .unwrap();
    }
    output
}
