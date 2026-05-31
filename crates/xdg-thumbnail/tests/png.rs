// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use xdg_thumbnail::{
    CacheEntryProblem, OriginalIdentity, ParsedThumbnailPng, PersonalOriginalUri,
    PersonalValidationOutcome, ReadableOriginalIdentity, SharedOriginalMetadata,
    SharedRepositoryContext, SharedValidationOutcome, ThumbnailError, ThumbnailPngBitDepth,
    ThumbnailPngColorType, ThumbnailSize, UnixMTimeSeconds, validate_personal_thumbnail,
    validate_shared_thumbnail,
};

#[test]
fn parses_standard_thumbnail_metadata() {
    let png = png_with_metadata(2, 1, png::ColorType::Rgba, metadata());

    let parsed = ParsedThumbnailPng::parse(&png).unwrap();

    assert_eq!(parsed.width(), 2);
    assert_eq!(parsed.height(), 1);
    assert_eq!(parsed.bit_depth(), ThumbnailPngBitDepth::Eight);
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
    assert_eq!(
        parsed.metadata().thumb_uri(),
        Some("file:///home/alice/photo.png")
    );
    assert_eq!(
        parsed.metadata().thumb_mtime(),
        Some(UnixMTimeSeconds::new(42))
    );
    assert_eq!(
        parsed.metadata().try_thumb_mtime().unwrap(),
        Some(UnixMTimeSeconds::new(42))
    );
    assert_eq!(parsed.metadata().thumb_size(), Some(12));
    assert_eq!(parsed.metadata().try_thumb_size().unwrap(), Some(12));
    assert_eq!(parsed.metadata().thumb_mime_type(), Some("image/png"));
    assert_eq!(
        parsed.metadata().iter().collect::<BTreeMap<_, _>>(),
        BTreeMap::from([
            ("Thumb::MTime", "42"),
            ("Thumb::Mimetype", "image/png"),
            ("Thumb::Size", "12"),
            ("Thumb::URI", "file:///home/alice/photo.png"),
        ])
    );
}

#[test]
fn metadata_typed_accessors_distinguish_invalid_syntax() {
    let mut metadata = metadata();
    metadata.insert("Thumb::MTime", "-1");
    metadata.insert("Thumb::Size", "not-a-size");
    let png = png_with_metadata(2, 1, png::ColorType::Rgba, metadata);
    let parsed = ParsedThumbnailPng::parse(&png).unwrap();

    assert_eq!(parsed.metadata().thumb_mtime(), None);
    assert!(parsed.metadata().try_thumb_mtime().is_err());
    assert_eq!(parsed.metadata().thumb_size(), None);
    assert!(parsed.metadata().try_thumb_size().is_err());
}

#[test]
fn validates_personal_thumbnail_metadata_and_conformance() {
    let original = ReadableOriginalIdentity::from_confirmed_readable_identity(original_identity());
    let valid = png_with_metadata(2, 1, png::ColorType::Rgba, metadata());
    assert_eq!(
        validate_personal_thumbnail(&valid, &original, ThumbnailSize::Normal),
        PersonalValidationOutcome::FullyVerified
    );

    let mut missing_uri = metadata();
    missing_uri.remove("Thumb::URI");
    assert_personal_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, missing_uri),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::MissingRequiredMetadata,
    );

    let mut bad_mtime = metadata();
    bad_mtime.insert("Thumb::MTime", "not-an-int");
    assert_personal_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, bad_mtime),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::InvalidMetadataSyntax,
    );

    let mut negative_mtime = metadata();
    negative_mtime.insert("Thumb::MTime", "-1");
    assert_personal_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, negative_mtime),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::InvalidMetadataSyntax,
    );

    let mut stale = metadata();
    stale.insert("Thumb::MTime", "41");
    assert_personal_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, stale),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::StaleMetadata,
    );

    let mut invalid_uri = metadata();
    invalid_uri.insert("Thumb::URI", "file:///home/alice/My Photo.png");
    assert_personal_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, invalid_uri),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::InvalidMetadataSyntax,
    );

    assert_personal_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgb, metadata()),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::NonconformingPngFormat,
    );

    assert_personal_invalid_contains(
        validate_personal_thumbnail(
            &png_with_metadata(129, 1, png::ColorType::Rgba, metadata()),
            &original,
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::DimensionsExceedNamespace,
    );

    assert_personal_invalid_contains(
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
            shared_original_metadata(),
            ThumbnailSize::Normal
        ),
        SharedValidationOutcome::MetadataIncomplete
    );

    let mut mismatched = BTreeMap::new();
    mismatched.insert("Thumb::URI", "./other.png");
    assert_shared_invalid_contains(
        validate_shared_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, mismatched),
            &context,
            shared_original_metadata(),
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::StaleMetadata,
    );

    let mut invalid_uri = BTreeMap::new();
    invalid_uri.insert("Thumb::URI", "./My Photo.png");
    assert_shared_invalid_contains(
        validate_shared_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, invalid_uri),
            &context,
            shared_original_metadata(),
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::InvalidMetadataSyntax,
    );

    let mut negative_mtime = BTreeMap::new();
    negative_mtime.insert("Thumb::URI", "./picture.png");
    negative_mtime.insert("Thumb::MTime", "-1");
    assert_shared_invalid_contains(
        validate_shared_thumbnail(
            &png_with_metadata(2, 1, png::ColorType::Rgba, negative_mtime),
            &context,
            shared_original_metadata(),
            ThumbnailSize::Normal,
        ),
        CacheEntryProblem::InvalidMetadataSyntax,
    );
}

fn shared_original_metadata() -> SharedOriginalMetadata {
    SharedOriginalMetadata::new()
        .with_mtime(UnixMTimeSeconds::new(42))
        .with_original_byte_size(12)
}

fn original_identity() -> OriginalIdentity {
    OriginalIdentity::new(
        PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
        UnixMTimeSeconds::new(42),
    )
    .with_original_byte_size(12)
    .with_mime_type("image/png")
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

#[test]
fn parser_and_validation_reject_png_resource_limits() {
    let png = png_header_only(4097, 4097, png::ColorType::Rgba);

    assert!(matches!(
        ParsedThumbnailPng::parse(&png),
        Err(ThumbnailError::ResourceLimitExceeded(_))
    ));

    let original = ReadableOriginalIdentity::from_confirmed_readable_identity(original_identity());
    assert_personal_invalid_contains(
        validate_personal_thumbnail(&png, &original, ThumbnailSize::Normal),
        CacheEntryProblem::ResourceLimitExceeded,
    );
}

fn assert_personal_invalid_contains(
    outcome: PersonalValidationOutcome,
    problem: CacheEntryProblem,
) {
    match outcome {
        PersonalValidationOutcome::Invalid(problems) => {
            assert!(problems.contains(&problem), "{problems:?}")
        }
        other => panic!("expected invalid outcome, got {other:?}"),
    }
}

fn assert_shared_invalid_contains(outcome: SharedValidationOutcome, problem: CacheEntryProblem) {
    match outcome {
        SharedValidationOutcome::Invalid(problems) => {
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

fn png_header_only(width: u32, height: u32, color_type: png::ColorType) -> Vec<u8> {
    let color_type = match color_type {
        png::ColorType::Grayscale => 0,
        png::ColorType::Rgb => 2,
        png::ColorType::Indexed => 3,
        png::ColorType::GrayscaleAlpha => 4,
        png::ColorType::Rgba => 6,
    };
    let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]);
    append_png_chunk(&mut output, b"IHDR", &ihdr);
    append_png_chunk(
        &mut output,
        b"IDAT",
        &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    );
    append_png_chunk(&mut output, b"IEND", &[]);
    output
}

fn append_png_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);
    output.extend_from_slice(&png_crc(chunk_type, data).to_be_bytes());
}

fn png_crc(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in chunk_type.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}
