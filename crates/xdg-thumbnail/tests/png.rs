// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use xdg_thumbnail::{
    CacheEntryProblem, OriginalIdentity, ParsedThumbnailPng, PersonalThumbnailUri,
    ReadableOriginalIdentity, SharedRepositoryContext, ThumbnailSize, UnixMTimeSeconds,
    ValidationOutcome, validate_personal_thumbnail, validate_shared_thumbnail,
};

#[test]
fn parses_standard_thumbnail_metadata() {
    let png = png_with_metadata(2, 1, png::ColorType::Rgba, metadata());

    let parsed = ParsedThumbnailPng::parse(&png).unwrap();

    assert_eq!(parsed.width(), 2);
    assert_eq!(parsed.height(), 1);
    assert_eq!(
        parsed.metadata().thumb_uri(),
        Some("file:///home/alice/photo.png")
    );
    assert_eq!(parsed.metadata().thumb_mtime(), Some(42));
    assert_eq!(parsed.metadata().thumb_size(), Some(12));
    assert_eq!(parsed.metadata().thumb_mimetype(), Some("image/png"));
}

#[test]
fn validates_personal_thumbnail_metadata_and_conformance() {
    let original = ReadableOriginalIdentity::new(original_identity());
    let valid = png_with_metadata(2, 1, png::ColorType::Rgba, metadata());
    assert_eq!(
        validate_personal_thumbnail(&valid, &original, ThumbnailSize::Normal),
        ValidationOutcome::FullyVerified
    );

    let mut missing_uri = metadata();
    missing_uri.remove("Thumb::URI");
    assert_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, missing_uri),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::MissingRequiredMetadata,
    );

    let mut bad_mtime = metadata();
    bad_mtime.insert("Thumb::MTime", "not-an-int");
    assert_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, bad_mtime),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::InvalidMetadataSyntax,
    );

    let mut stale = metadata();
    stale.insert("Thumb::MTime", "41");
    assert_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, stale),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::StaleMetadata,
    );

    let mut invalid_uri = metadata();
    invalid_uri.insert("Thumb::URI", "file:///home/alice/My Photo.png");
    assert_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, invalid_uri),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::InvalidMetadataSyntax,
    );

    assert_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgb, metadata()),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::NonconformingPngFormat,
    );

    assert_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(129, 1, png::ColorType::Rgba, metadata()),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::DimensionsExceedNamespace,
    );

    assert_invalid_contains(
        validate_personal_thumbnail(b"not png", &original, ThumbnailSize::Normal),
        CacheEntryProblem::InvalidPngStructure,
    );
}

#[test]
fn shared_validation_allows_incomplete_freshness_metadata_explicitly() {
    let context =
        SharedRepositoryContext::new(Path::new("/srv/photos"), OsStr::from_bytes(b"picture.png"))
            .unwrap();

    let incomplete = png_with_metadata(2, 1, png::ColorType::Rgba, BTreeMap::new());
    assert_eq!(
        validate_shared_thumbnail(
            &incomplete,
            &context,
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
            ThumbnailSize::Normal
        ),
        ValidationOutcome::SharedMetadataIncomplete
    );

    let mut mismatched = BTreeMap::new();
    mismatched.insert("Thumb::URI", "./other.png");
    assert_invalid_contains(
        validate_shared_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, mismatched),
            &context,
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::StaleMetadata,
    );

    let mut invalid_uri = BTreeMap::new();
    invalid_uri.insert("Thumb::URI", "./My Photo.png");
    assert_invalid_contains(
        validate_shared_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, invalid_uri),
            &context,
            Some(UnixMTimeSeconds::new(42)),
            Some(12),
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::InvalidMetadataSyntax,
    );
}

fn original_identity() -> OriginalIdentity {
    OriginalIdentity::new(
        PersonalThumbnailUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
        UnixMTimeSeconds::new(42),
        Some(12),
        Some("image/png"),
    )
    .unwrap()
}

fn metadata() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("Thumb::URI", "file:///home/alice/photo.png"),
        ("Thumb::MTime", "42"),
        ("Thumb::Size", "12"),
        ("Thumb::Mimetype", "image/png"),
    ])
}

fn assert_invalid_contains(outcome: ValidationOutcome, problem: CacheEntryProblem) {
    match outcome {
        ValidationOutcome::Invalid(problems) => {
            assert!(problems.contains(&problem), "{problems:?}")
        }
        other => panic!("expected invalid outcome, got {other:?}"),
    }
}

fn png_with_metadata(
    width: u32,
    height: u32,
    color_type: png::ColorType,
    metadata: BTreeMap<&str, &str>,
) -> Vec<u8> {
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
