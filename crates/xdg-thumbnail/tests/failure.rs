// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, CacheNamespace, FailureNamespace, ParsedThumbnailPng, PersonalCacheRoot,
    PersonalOriginalIdentity, PersonalOriginalUri, PersonalValidationOutcome,
    ReadablePersonalOriginalIdentity, ThumbnailMetadataKey, ThumbnailMetadataProblem,
    ThumbnailMetadataProblemKind, ThumbnailPngColorType, UnixMtimeSeconds,
    validate_personal_failure_entry,
};

#[test]
fn writes_deterministic_failure_namespace_entries() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let namespace = FailureNamespace::new("xdg-thumbnail-0.1.0").unwrap();
    let original = readable_original();

    let first = root
        .write_failure_entry_png_bytes(&original, &namespace)
        .unwrap();
    let second = root
        .write_failure_entry_png_bytes(&original, &namespace)
        .unwrap();

    let expected_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Failure(namespace.clone()),
    );
    assert_eq!(first.path(), expected_path.as_path());
    assert_eq!(first.png_bytes(), second.png_bytes());
    assert_eq!(std::fs::read(&expected_path).unwrap(), first.png_bytes());

    let parsed = ParsedThumbnailPng::parse(first.png_bytes()).unwrap();
    assert_eq!(parsed.width(), 1);
    assert_eq!(parsed.height(), 1);
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
    assert_eq!(parsed.metadata().thumb_mime_type(), Some("image/png"));
    assert_eq!(
        validate_personal_failure_entry(first.png_bytes(), &original),
        PersonalValidationOutcome::FullyVerified
    );
}

#[test]
fn failure_path_variant_returns_only_installed_path() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let namespace = FailureNamespace::new("xdg-thumbnail-0.1.0").unwrap();
    let original = readable_original();

    let installed = root
        .write_failure_entry_path(&original, &namespace)
        .unwrap();
    let expected_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Failure(namespace),
    );

    assert_eq!(installed.path(), expected_path.as_path());
    assert!(expected_path.exists());
}

#[test]
fn validates_failure_entry_metadata_without_successful_thumbnail_size_limits() {
    let original = readable_original();
    let bytes = failure_png_with_metadata(2048, 1, original.identity());

    assert_eq!(
        validate_personal_failure_entry(&bytes, &original),
        PersonalValidationOutcome::FullyVerified
    );
}

#[test]
fn reports_stale_failure_entry_metadata() {
    let original = readable_original();
    let bytes = failure_png_with_metadata(1, 1, original.identity());
    let stale_original = ReadablePersonalOriginalIdentity::assume_readable(
        PersonalOriginalIdentity::new(original.identity().uri().clone(), UnixMtimeSeconds::new(43))
            .with_original_byte_size(12)
            .with_mime_type("image/png")
            .unwrap(),
    );

    assert_eq!(
        validate_personal_failure_entry(&bytes, &stale_original),
        PersonalValidationOutcome::Invalid(vec![metadata_problem(
            ThumbnailMetadataKey::Mtime,
            ThumbnailMetadataProblemKind::ValueMismatch,
        )])
    );
}

fn metadata_problem(
    key: ThumbnailMetadataKey,
    kind: ThumbnailMetadataProblemKind,
) -> CacheEntryProblem {
    CacheEntryProblem::Metadata(ThumbnailMetadataProblem::new(key, kind))
}

fn readable_original() -> ReadablePersonalOriginalIdentity {
    ReadablePersonalOriginalIdentity::assume_readable(
        PersonalOriginalIdentity::new(
            PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            UnixMtimeSeconds::new(42),
        )
        .with_original_byte_size(12)
        .with_mime_type("image/png")
        .unwrap(),
    )
}

fn failure_png_with_metadata(
    width: u32,
    height: u32,
    original: &PersonalOriginalIdentity,
) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_text_chunk("Thumb::URI".to_owned(), original.uri().as_str().to_owned())
            .unwrap();
        encoder
            .add_text_chunk("Thumb::MTime".to_owned(), original.mtime().to_string())
            .unwrap();
        if let Some(size) = original.original_byte_size() {
            encoder
                .add_text_chunk("Thumb::Size".to_owned(), size.to_string())
                .unwrap();
        }
        if let Some(mime_type) = original.mime_type() {
            encoder
                .add_text_chunk("Thumb::Mimetype".to_owned(), mime_type.to_owned())
                .unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![0; width as usize * height as usize * 4])
            .unwrap();
    }
    output
}
