// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::io::Cursor;

use crate::uri::validate_absolute_uri_identity;
use crate::{
    OriginalIdentity, ReadableOriginalIdentity, Result, SharedRelativeOriginalUri,
    SharedRepositoryContext, ThumbnailError, ThumbnailSize, UnixMTimeSeconds,
};

const MAX_RENDERED_PIXELS: u64 = 16_777_216;
const MAX_RENDERED_DECODE_BYTES: usize = 256 * 1024 * 1024;

/// Explicit raw pixel format accepted by raw thumbnail install APIs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum RawThumbnailPixelFormat {
    /// Three 8-bit channels per pixel in red, green, blue order.
    Rgb8,
    /// Four 8-bit channels per pixel in red, green, blue, alpha order.
    Rgba8,
}

impl RawThumbnailPixelFormat {
    const fn channels(self) -> usize {
        match self {
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }
}

/// Borrowed raw rendered thumbnail pixels.
///
/// This type validates the caller-supplied dimensions, stride, format, and buffer length. Raw
/// thumbnail install APIs do not infer pixel format from byte length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawThumbnailImage<'a> {
    width: u32,
    height: u32,
    stride: usize,
    format: RawThumbnailPixelFormat,
    pixels: &'a [u8],
}

impl<'a> RawThumbnailImage<'a> {
    /// Creates a validated borrowed raw thumbnail image.
    ///
    /// # Errors
    ///
    /// Returns an error when dimensions are zero or exceed resource limits, stride is too small,
    /// the supplied buffer is too short, or required size arithmetic overflows.
    pub fn new(
        width: u32,
        height: u32,
        stride: usize,
        format: RawThumbnailPixelFormat,
        pixels: &'a [u8],
    ) -> Result<Self> {
        validate_raw_thumbnail_image(width, height, stride, format, pixels)?;
        Ok(Self {
            width,
            height,
            stride,
            format,
            pixels,
        })
    }

    /// Returns the image width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the image height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the row stride in bytes.
    #[must_use]
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the explicit pixel format.
    #[must_use]
    pub const fn format(&self) -> RawThumbnailPixelFormat {
        self.format
    }

    /// Returns the validated pixel buffer.
    #[must_use]
    pub const fn pixels(&self) -> &'a [u8] {
        self.pixels
    }
}

/// Owned raw rendered thumbnail pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedRawThumbnailImage {
    width: u32,
    height: u32,
    stride: usize,
    format: RawThumbnailPixelFormat,
    pixels: Vec<u8>,
}

impl OwnedRawThumbnailImage {
    /// Creates a validated owned raw thumbnail image.
    ///
    /// # Errors
    ///
    /// Returns an error when dimensions are zero or exceed resource limits, stride is too small,
    /// the supplied buffer is too short, or required size arithmetic overflows.
    pub fn new(
        width: u32,
        height: u32,
        stride: usize,
        format: RawThumbnailPixelFormat,
        pixels: Vec<u8>,
    ) -> Result<Self> {
        validate_raw_thumbnail_image(width, height, stride, format, &pixels)?;
        Ok(Self {
            width,
            height,
            stride,
            format,
            pixels,
        })
    }

    /// Borrows this owned image for raw install APIs.
    #[must_use]
    pub fn as_borrowed(&self) -> RawThumbnailImage<'_> {
        RawThumbnailImage {
            width: self.width,
            height: self.height,
            stride: self.stride,
            format: self.format,
            pixels: &self.pixels,
        }
    }

    /// Returns the image width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the image height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the row stride in bytes.
    #[must_use]
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the explicit pixel format.
    #[must_use]
    pub const fn format(&self) -> RawThumbnailPixelFormat {
        self.format
    }

    /// Returns the validated pixel buffer.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Splits this image into its validated owned parts.
    #[must_use]
    pub fn into_parts(self) -> (u32, u32, usize, RawThumbnailPixelFormat, Vec<u8>) {
        (
            self.width,
            self.height,
            self.stride,
            self.format,
            self.pixels,
        )
    }
}

/// Policy-neutral problem found while validating or inspecting a cache entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CacheEntryProblem {
    /// Metadata is well-formed but no longer matches the original.
    StaleMetadata,
    /// The cache entry itself could not be read well enough to validate.
    UnreadableEntry,
    /// The original cannot be verified in this validation context.
    UnverifiableOriginal,
    /// Required metadata is missing for this validation context.
    MissingRequiredMetadata,
    /// Present metadata has invalid syntax.
    InvalidMetadataSyntax,
    /// PNG structure could not be decoded.
    InvalidPngStructure,
    /// PNG encoding does not conform to the successful-thumbnail requirements.
    NonconformingPngFormat,
    /// PNG dimensions exceed the requested namespace.
    DimensionsExceedNamespace,
    /// PNG decoding would exceed configured resource limits.
    ResourceLimitExceeded,
    /// The cache directory entry is not a standard thumbnail filename.
    NonstandardFilename,
    /// The standard cache filename does not match the stored thumbnail URI identity.
    UriFilenameMismatch,
}

/// Validation confidence and validity for a personal-cache entry.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PersonalValidationOutcome {
    /// Required metadata and PNG constraints are fully verified.
    FullyVerified,
    /// The entry is invalid for the requested validation context.
    Invalid(Vec<CacheEntryProblem>),
}

/// Validation confidence and validity for a shared-repository entry.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SharedValidationOutcome {
    /// Required metadata and PNG constraints are fully verified.
    FullyVerified,
    /// Shared thumbnail metadata is standard-allowed but incomplete.
    MetadataIncomplete,
    /// The entry is invalid for the requested validation context.
    Invalid(Vec<CacheEntryProblem>),
}

/// Freedesktop thumbnail PNG text metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ThumbnailMetadata {
    values: BTreeMap<String, String>,
}

impl ThumbnailMetadata {
    /// Returns a raw metadata value by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Returns `Thumb::URI` when present.
    #[must_use]
    pub fn thumb_uri(&self) -> Option<&str> {
        self.get("Thumb::URI")
    }

    /// Returns parsed `Thumb::MTime` when present and syntactically valid.
    #[must_use]
    pub fn thumb_mtime(&self) -> Option<UnixMTimeSeconds> {
        self.try_thumb_mtime().ok().flatten()
    }

    /// Returns parsed `Thumb::MTime`, distinguishing missing metadata from invalid syntax.
    ///
    /// # Errors
    ///
    /// Returns an error when `Thumb::MTime` is present but not a non-negative whole Unix epoch
    /// second value.
    pub fn try_thumb_mtime(&self) -> Result<Option<UnixMTimeSeconds>> {
        self.get("Thumb::MTime").map(parse_thumb_mtime).transpose()
    }

    pub(crate) fn thumb_mtime_result(&self) -> Result<Option<UnixMTimeSeconds>> {
        self.try_thumb_mtime()
    }

    /// Returns parsed `Thumb::Size` when present and syntactically valid.
    #[must_use]
    pub fn thumb_size(&self) -> Option<u64> {
        self.try_thumb_size().ok().flatten()
    }

    /// Returns parsed `Thumb::Size`, distinguishing missing metadata from invalid syntax.
    ///
    /// # Errors
    ///
    /// Returns an error when `Thumb::Size` is present but is not an unsigned integer.
    pub fn try_thumb_size(&self) -> Result<Option<u64>> {
        self.get("Thumb::Size").map(parse_thumb_size).transpose()
    }

    pub(crate) fn thumb_size_result(&self) -> Result<Option<u64>> {
        self.try_thumb_size()
    }

    /// Returns `Thumb::Mimetype` when present.
    #[must_use]
    pub fn thumb_mimetype(&self) -> Option<&str> {
        self.get("Thumb::Mimetype")
    }
}

fn parse_thumb_mtime(value: &str) -> Result<UnixMTimeSeconds> {
    value
        .parse::<u64>()
        .map(UnixMTimeSeconds::new)
        .map_err(|_| ThumbnailError::InvalidMetadata("invalid Thumb::MTime"))
}

fn parse_thumb_size(value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .map_err(|_| ThumbnailError::InvalidMetadata("invalid Thumb::Size"))
}

/// PNG sample bit depth reported by [`ParsedThumbnailPng`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ThumbnailPngBitDepth {
    /// 1-bit samples.
    One,
    /// 2-bit samples.
    Two,
    /// 4-bit samples.
    Four,
    /// 8-bit samples.
    Eight,
    /// 16-bit samples.
    Sixteen,
}

impl ThumbnailPngBitDepth {
    fn from_png(value: png::BitDepth) -> Self {
        match value {
            png::BitDepth::One => Self::One,
            png::BitDepth::Two => Self::Two,
            png::BitDepth::Four => Self::Four,
            png::BitDepth::Eight => Self::Eight,
            png::BitDepth::Sixteen => Self::Sixteen,
        }
    }
}

/// PNG color type reported by [`ParsedThumbnailPng`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ThumbnailPngColorType {
    /// Grayscale samples without alpha.
    Grayscale,
    /// RGB samples without alpha.
    Rgb,
    /// Indexed-color samples.
    Indexed,
    /// Grayscale samples with alpha.
    GrayscaleAlpha,
    /// RGBA samples.
    Rgba,
}

impl ThumbnailPngColorType {
    fn from_png(value: png::ColorType) -> Self {
        match value {
            png::ColorType::Grayscale => Self::Grayscale,
            png::ColorType::Rgb => Self::Rgb,
            png::ColorType::Indexed => Self::Indexed,
            png::ColorType::GrayscaleAlpha => Self::GrayscaleAlpha,
            png::ColorType::Rgba => Self::Rgba,
        }
    }
}

/// Decoded facts from a thumbnail PNG.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedThumbnailPng {
    width: u32,
    height: u32,
    bit_depth: ThumbnailPngBitDepth,
    color_type: ThumbnailPngColorType,
    interlaced: bool,
    metadata: ThumbnailMetadata,
}

impl ParsedThumbnailPng {
    /// Parses PNG structure and Freedesktop text metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when PNG decoding fails, text metadata cannot be decoded, or decoding would
    /// exceed the crate's pixel or output-buffer resource limits.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let decoder = png::Decoder::new(Cursor::new(bytes));
        let mut reader = decoder
            .read_info()
            .map_err(|err| ThumbnailError::Png(err.to_string()))?;
        let Some(output_buffer_size) = reader.output_buffer_size() else {
            return Err(ThumbnailError::Png(
                "png output buffer size is unavailable".to_owned(),
            ));
        };
        let info = reader.info();
        ensure_parsed_png_resource_limits(info.width, info.height, output_buffer_size)?;
        let mut buffer = vec![0; output_buffer_size];
        reader
            .next_frame(&mut buffer)
            .map_err(|err| ThumbnailError::Png(err.to_string()))?;

        let info = reader.info();
        let mut values = BTreeMap::new();
        for chunk in &info.uncompressed_latin1_text {
            values.insert(chunk.keyword.clone(), chunk.text.clone());
        }
        for chunk in &info.compressed_latin1_text {
            let text = chunk
                .get_text()
                .map_err(|err| ThumbnailError::Png(err.to_string()))?;
            values.insert(chunk.keyword.clone(), text);
        }
        for chunk in &info.utf8_text {
            let text = chunk
                .get_text()
                .map_err(|err| ThumbnailError::Png(err.to_string()))?;
            values.insert(chunk.keyword.clone(), text);
        }

        Ok(Self {
            width: info.width,
            height: info.height,
            bit_depth: ThumbnailPngBitDepth::from_png(info.bit_depth),
            color_type: ThumbnailPngColorType::from_png(info.color_type),
            interlaced: info.interlaced,
            metadata: ThumbnailMetadata { values },
        })
    }

    /// Returns the image width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the image height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the image bit depth.
    #[must_use]
    pub const fn bit_depth(&self) -> ThumbnailPngBitDepth {
        self.bit_depth
    }

    /// Returns the image color type.
    #[must_use]
    pub const fn color_type(&self) -> ThumbnailPngColorType {
        self.color_type
    }

    /// Returns whether the PNG is interlaced.
    #[must_use]
    pub const fn interlaced(&self) -> bool {
        self.interlaced
    }

    /// Returns parsed Freedesktop thumbnail metadata.
    #[must_use]
    pub const fn metadata(&self) -> &ThumbnailMetadata {
        &self.metadata
    }

    pub(crate) fn into_metadata(self) -> ThumbnailMetadata {
        self.metadata
    }

    pub(crate) fn conformance_problems(&self, size: ThumbnailSize) -> Vec<CacheEntryProblem> {
        let mut problems = Vec::new();
        if self.bit_depth != ThumbnailPngBitDepth::Eight
            || self.interlaced
            || !matches!(
                self.color_type,
                ThumbnailPngColorType::Rgba | ThumbnailPngColorType::GrayscaleAlpha
            )
        {
            push_problem(&mut problems, CacheEntryProblem::NonconformingPngFormat);
        }
        if self.width > size.max_dimension() || self.height > size.max_dimension() {
            push_problem(&mut problems, CacheEntryProblem::DimensionsExceedNamespace);
        }
        problems
    }
}

fn ensure_parsed_png_resource_limits(
    width: u32,
    height: u32,
    output_buffer_size: usize,
) -> Result<()> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_RENDERED_PIXELS || output_buffer_size > MAX_RENDERED_DECODE_BYTES {
        return Err(ThumbnailError::ResourceLimitExceeded(
            "PNG decode resource limit exceeded",
        ));
    }
    Ok(())
}

fn parse_thumbnail_for_validation(
    bytes: &[u8],
) -> std::result::Result<ParsedThumbnailPng, CacheEntryProblem> {
    match ParsedThumbnailPng::parse(bytes) {
        Ok(parsed) => Ok(parsed),
        Err(ThumbnailError::ResourceLimitExceeded(_)) => {
            Err(CacheEntryProblem::ResourceLimitExceeded)
        }
        Err(_) => Err(CacheEntryProblem::InvalidPngStructure),
    }
}

/// Validates a personal-cache successful thumbnail PNG against a readable original identity.
#[must_use]
pub fn validate_personal_thumbnail(
    bytes: &[u8],
    original: &ReadableOriginalIdentity,
    size: ThumbnailSize,
) -> PersonalValidationOutcome {
    validate_personal_thumbnail_identity(bytes, original.identity(), size)
}

pub(crate) fn validate_personal_thumbnail_identity(
    bytes: &[u8],
    original: &OriginalIdentity,
    size: ThumbnailSize,
) -> PersonalValidationOutcome {
    let parsed = match parse_thumbnail_for_validation(bytes) {
        Ok(parsed) => parsed,
        Err(problem) => {
            return PersonalValidationOutcome::Invalid(vec![problem]);
        }
    };

    let mut problems = parsed.conformance_problems(size);
    compare_personal_metadata(&mut problems, parsed.metadata(), original);

    if problems.is_empty() {
        PersonalValidationOutcome::FullyVerified
    } else {
        PersonalValidationOutcome::Invalid(problems)
    }
}

/// Validates a personal-cache failure entry PNG against a readable original identity.
///
/// Failure entries carry the same required personal-cache metadata as successful thumbnails, but
/// they are not successful-thumbnail size entries and are not checked against size-class dimension
/// limits.
#[must_use]
pub fn validate_personal_failure_entry(
    bytes: &[u8],
    original: &ReadableOriginalIdentity,
) -> PersonalValidationOutcome {
    validate_personal_failure_entry_identity(bytes, original.identity())
}

pub(crate) fn validate_personal_failure_entry_identity(
    bytes: &[u8],
    original: &OriginalIdentity,
) -> PersonalValidationOutcome {
    let parsed = match parse_thumbnail_for_validation(bytes) {
        Ok(parsed) => parsed,
        Err(problem) => {
            return PersonalValidationOutcome::Invalid(vec![problem]);
        }
    };

    let mut problems = Vec::new();
    compare_personal_metadata(&mut problems, parsed.metadata(), original);

    if problems.is_empty() {
        PersonalValidationOutcome::FullyVerified
    } else {
        PersonalValidationOutcome::Invalid(problems)
    }
}

/// Validates a shared-repository successful thumbnail PNG against explicit shared context.
#[must_use]
pub fn validate_shared_thumbnail(
    bytes: &[u8],
    context: &SharedRepositoryContext,
    mtime: Option<UnixMTimeSeconds>,
    original_byte_size: Option<u64>,
    size: ThumbnailSize,
) -> SharedValidationOutcome {
    let parsed = match parse_thumbnail_for_validation(bytes) {
        Ok(parsed) => parsed,
        Err(problem) => {
            return SharedValidationOutcome::Invalid(vec![problem]);
        }
    };

    let mut problems = parsed.conformance_problems(size);
    let mut incomplete = false;
    let metadata = parsed.metadata();

    match metadata.thumb_uri() {
        Some(uri) if uri == context.shared_uri().as_str() => {}
        Some(uri) if SharedRelativeOriginalUri::parse(uri).is_err() => {
            push_problem(&mut problems, CacheEntryProblem::InvalidMetadataSyntax);
        }
        Some(_) => push_problem(&mut problems, CacheEntryProblem::StaleMetadata),
        None => incomplete = true,
    }

    match (metadata.thumb_mtime_result(), mtime) {
        (Ok(Some(stored)), Some(expected)) if stored == expected => {}
        (Ok(Some(_)), Some(_)) => push_problem(&mut problems, CacheEntryProblem::StaleMetadata),
        (Ok(Some(_)), None) => push_problem(&mut problems, CacheEntryProblem::UnverifiableOriginal),
        (Ok(None), _) => incomplete = true,
        (Err(_), _) => push_problem(&mut problems, CacheEntryProblem::InvalidMetadataSyntax),
    }

    compare_optional_size(&mut problems, metadata, original_byte_size);

    if !problems.is_empty() {
        SharedValidationOutcome::Invalid(problems)
    } else if incomplete {
        SharedValidationOutcome::MetadataIncomplete
    } else {
        SharedValidationOutcome::FullyVerified
    }
}

fn compare_personal_metadata(
    problems: &mut Vec<CacheEntryProblem>,
    metadata: &ThumbnailMetadata,
    original: &OriginalIdentity,
) {
    match metadata.thumb_uri() {
        Some(uri) if uri == original.uri().as_str() => {}
        Some(uri) if validate_absolute_uri_identity(uri).is_err() => {
            push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax);
        }
        Some(_) => push_problem(problems, CacheEntryProblem::StaleMetadata),
        None => push_problem(problems, CacheEntryProblem::MissingRequiredMetadata),
    }

    match metadata.thumb_mtime_result() {
        Ok(Some(mtime)) if mtime == original.mtime() => {}
        Ok(Some(_)) => push_problem(problems, CacheEntryProblem::StaleMetadata),
        Ok(None) => push_problem(problems, CacheEntryProblem::MissingRequiredMetadata),
        Err(_) => push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax),
    }

    compare_optional_size(problems, metadata, original.original_byte_size());
    compare_optional_mimetype(problems, metadata, original.mime_type());
}

fn compare_optional_size(
    problems: &mut Vec<CacheEntryProblem>,
    metadata: &ThumbnailMetadata,
    expected: Option<u64>,
) {
    match (metadata.thumb_size_result(), expected) {
        (Ok(Some(stored)), Some(expected)) if stored == expected => {}
        (Ok(Some(_)), Some(_)) => push_problem(problems, CacheEntryProblem::StaleMetadata),
        (Ok(_), _) => {}
        (Err(_), _) => push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax),
    }
}

fn compare_optional_mimetype(
    problems: &mut Vec<CacheEntryProblem>,
    metadata: &ThumbnailMetadata,
    expected: Option<&str>,
) {
    let Some(stored) = metadata.thumb_mimetype() else {
        return;
    };
    if validate_mime_type(stored).is_err() {
        push_problem(problems, CacheEntryProblem::InvalidMetadataSyntax);
    } else if let Some(expected) = expected {
        if stored != expected {
            push_problem(problems, CacheEntryProblem::StaleMetadata);
        }
    }
}

pub(crate) fn push_problem(problems: &mut Vec<CacheEntryProblem>, problem: CacheEntryProblem) {
    if !problems.contains(&problem) {
        problems.push(problem);
    }
}

struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

pub(crate) fn normalized_personal_thumbnail_png(
    rendered_png: &[u8],
    original: &OriginalIdentity,
    size: ThumbnailSize,
) -> Result<Vec<u8>> {
    let image = decode_rendered_png_to_rgba8(rendered_png)?;
    normalized_personal_thumbnail_rgba_png(image, original, size)
}

pub(crate) fn normalized_personal_thumbnail_raw_png(
    image: RawThumbnailImage<'_>,
    original: &OriginalIdentity,
    size: ThumbnailSize,
) -> Result<Vec<u8>> {
    let image = raw_thumbnail_to_rgba8(image)?;
    normalized_personal_thumbnail_rgba_png(image, original, size)
}

fn normalized_personal_thumbnail_rgba_png(
    image: RgbaImage,
    original: &OriginalIdentity,
    size: ThumbnailSize,
) -> Result<Vec<u8>> {
    let image = downscale_to_namespace(image, size)?;
    let metadata = thumbnail_metadata_pairs(original);
    let png = encode_rgba_png(image.width, image.height, &image.pixels, &metadata)?;
    match validate_personal_thumbnail_identity(&png, original, size) {
        PersonalValidationOutcome::FullyVerified => Ok(png),
        PersonalValidationOutcome::Invalid(problems) => {
            Err(ThumbnailError::UnsupportedRenderedThumbnail(
                rendered_validation_error(problems.as_slice()),
            ))
        }
    }
}

fn rendered_validation_error(problems: &[CacheEntryProblem]) -> &'static str {
    if problems.contains(&CacheEntryProblem::ResourceLimitExceeded) {
        "resource limit exceeded"
    } else if problems.contains(&CacheEntryProblem::DimensionsExceedNamespace) {
        "dimensions exceed namespace"
    } else if problems.contains(&CacheEntryProblem::NonconformingPngFormat) {
        "nonconforming final PNG"
    } else if problems.contains(&CacheEntryProblem::InvalidPngStructure) {
        "invalid final PNG structure"
    } else {
        "metadata validation failed"
    }
}

pub(crate) fn thumbnail_metadata_pairs(original: &OriginalIdentity) -> Vec<(String, String)> {
    let mut metadata = vec![
        ("Thumb::URI".to_owned(), original.uri().as_str().to_owned()),
        ("Thumb::MTime".to_owned(), original.mtime().to_string()),
    ];
    if let Some(original_byte_size) = original.original_byte_size() {
        metadata.push(("Thumb::Size".to_owned(), original_byte_size.to_string()));
    }
    if let Some(mime_type) = original.mime_type() {
        metadata.push(("Thumb::Mimetype".to_owned(), mime_type.to_owned()));
    }
    metadata
}

fn ensure_rendered_resource_limits(
    width: u32,
    height: u32,
    output_buffer_size: usize,
) -> Result<()> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_RENDERED_PIXELS || output_buffer_size > MAX_RENDERED_DECODE_BYTES {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "rendered PNG resource limit exceeded",
        ));
    }
    Ok(())
}

fn validate_raw_thumbnail_image(
    width: u32,
    height: u32,
    stride: usize,
    format: RawThumbnailPixelFormat,
    pixels: &[u8],
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "raw thumbnail dimensions must be non-zero",
        ));
    }

    let row_bytes = raw_row_bytes(width, format)?;
    let decoded_len = rgba_buffer_len(width, height)?;
    ensure_raw_resource_limits(width, height, decoded_len)?;

    if stride < row_bytes {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "raw thumbnail stride is too small",
        ));
    }

    let required_len = raw_required_buffer_len(height, stride, row_bytes)?;
    if pixels.len() < required_len {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "raw thumbnail buffer is too short",
        ));
    }

    Ok(())
}

fn ensure_raw_resource_limits(width: u32, height: u32, output_buffer_size: usize) -> Result<()> {
    ensure_rendered_resource_limits(width, height, output_buffer_size).map_err(
        |error| match error {
            ThumbnailError::UnsupportedRenderedThumbnail(_) => {
                ThumbnailError::UnsupportedRenderedThumbnail(
                    "raw thumbnail resource limit exceeded",
                )
            }
            error => error,
        },
    )
}

fn raw_row_bytes(width: u32, format: RawThumbnailPixelFormat) -> Result<usize> {
    usize::try_from(width)
        .map_err(|_| {
            ThumbnailError::UnsupportedRenderedThumbnail("raw thumbnail width overflows usize")
        })?
        .checked_mul(format.channels())
        .ok_or(ThumbnailError::UnsupportedRenderedThumbnail(
            "raw thumbnail row length overflows usize",
        ))
}

fn raw_required_buffer_len(height: u32, stride: usize, row_bytes: usize) -> Result<usize> {
    let height = usize::try_from(height).map_err(|_| {
        ThumbnailError::UnsupportedRenderedThumbnail("raw thumbnail height overflows usize")
    })?;
    stride
        .checked_mul(height.saturating_sub(1))
        .and_then(|bytes_before_last_row| bytes_before_last_row.checked_add(row_bytes))
        .ok_or(ThumbnailError::UnsupportedRenderedThumbnail(
            "raw thumbnail buffer length overflows usize",
        ))
}

fn pixel_count_len(width: u32, height: u32) -> Result<usize> {
    let pixels = u64::from(width) * u64::from(height);
    usize::try_from(pixels).map_err(|_| {
        ThumbnailError::UnsupportedRenderedThumbnail("rendered PNG dimensions are too large")
    })
}

fn rgba_buffer_len(width: u32, height: u32) -> Result<usize> {
    pixel_count_len(width, height)?.checked_mul(4).ok_or(
        ThumbnailError::UnsupportedRenderedThumbnail("RGBA buffer length overflows usize"),
    )
}

fn decode_rendered_png_to_rgba8(bytes: &[u8]) -> Result<RgbaImage> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|err| ThumbnailError::Png(err.to_string()))?;
    let info = reader.info();
    if info.animation_control.is_some() {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "animated PNG output is unsupported",
        ));
    }
    let Some(output_buffer_size) = reader.output_buffer_size() else {
        return Err(ThumbnailError::Png(
            "png output buffer size is unavailable".to_owned(),
        ));
    };
    ensure_rendered_resource_limits(info.width, info.height, output_buffer_size)?;
    let mut buffer = vec![0; output_buffer_size];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|err| ThumbnailError::Png(err.to_string()))?;
    let frame = &buffer[..output.buffer_size()];
    if output.bit_depth != png::BitDepth::Eight {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "decoded PNG did not normalize to 8-bit samples",
        ));
    }

    let pixels = match output.color_type {
        png::ColorType::Rgba => frame.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(rgba_buffer_len(output.width, output.height)?);
            for pixel in frame.chunks_exact(3) {
                out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(rgba_buffer_len(output.width, output.height)?);
            for pixel in frame.chunks_exact(2) {
                out.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
            out
        }
        png::ColorType::Grayscale | png::ColorType::Indexed => {
            let mut out = Vec::with_capacity(rgba_buffer_len(output.width, output.height)?);
            for &gray in frame {
                out.extend_from_slice(&[gray, gray, gray, 255]);
            }
            out
        }
    };

    Ok(RgbaImage {
        width: output.width,
        height: output.height,
        pixels,
    })
}

fn raw_thumbnail_to_rgba8(image: RawThumbnailImage<'_>) -> Result<RgbaImage> {
    let row_bytes = raw_row_bytes(image.width, image.format)?;
    let mut pixels = Vec::with_capacity(rgba_buffer_len(image.width, image.height)?);
    for row_index in 0..image.height as usize {
        let start = row_index * image.stride;
        let row = &image.pixels[start..start + row_bytes];
        match image.format {
            RawThumbnailPixelFormat::Rgb8 => {
                for pixel in row.chunks_exact(3) {
                    pixels.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
            }
            RawThumbnailPixelFormat::Rgba8 => pixels.extend_from_slice(row),
        }
    }
    Ok(RgbaImage {
        width: image.width,
        height: image.height,
        pixels,
    })
}

fn downscale_to_namespace(image: RgbaImage, size: ThumbnailSize) -> Result<RgbaImage> {
    let max = size.max_dimension();
    if image.width <= max && image.height <= max {
        return Ok(image);
    }
    let (width, height) = constrain_dimensions(image.width, image.height, max);
    let source = image
        .pixels
        .chunks_exact(4)
        .map(|pixel| resize::px::RGBA::new(pixel[0], pixel[1], pixel[2], pixel[3]))
        .collect::<Vec<_>>();
    let mut dest = vec![resize::px::RGBA::new(0, 0, 0, 0); pixel_count_len(width, height)?];
    let mut resizer = resize::new(
        image.width as usize,
        image.height as usize,
        width as usize,
        height as usize,
        resize::Pixel::RGBA8P,
        resize::Type::Lanczos3,
    )
    .map_err(|_| ThumbnailError::UnsupportedRenderedThumbnail("resize setup failed"))?;
    resizer
        .resize(&source, &mut dest)
        .map_err(|_| ThumbnailError::UnsupportedRenderedThumbnail("resize failed"))?;
    let mut pixels = Vec::with_capacity(dest.len() * 4);
    for pixel in dest {
        pixels.extend_from_slice(&[pixel.r, pixel.g, pixel.b, pixel.a]);
    }
    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

fn constrain_dimensions(width: u32, height: u32, max: u32) -> (u32, u32) {
    if width >= height {
        let scaled_height = (u64::from(height) * u64::from(max) / u64::from(width)).max(1) as u32;
        (max, scaled_height)
    } else {
        let scaled_width = (u64::from(width) * u64::from(max) / u64::from(height)).max(1) as u32;
        (scaled_width, max)
    }
}

pub(crate) fn encode_rgba_png(
    width: u32,
    height: u32,
    pixels: &[u8],
    metadata: &[(String, String)],
) -> Result<Vec<u8>> {
    let expected_len = rgba_buffer_len(width, height)?;
    if pixels.len() != expected_len {
        return Err(ThumbnailError::UnsupportedRenderedThumbnail(
            "RGBA buffer length does not match dimensions",
        ));
    }
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        for (key, value) in metadata {
            encoder
                .add_text_chunk(key.clone(), value.clone())
                .map_err(|err| ThumbnailError::Png(err.to_string()))?;
        }
        let mut writer = encoder
            .write_header()
            .map_err(|err| ThumbnailError::Png(err.to_string()))?;
        writer
            .write_image_data(pixels)
            .map_err(|err| ThumbnailError::Png(err.to_string()))?;
    }
    Ok(output)
}

pub(crate) fn validate_mime_type(mime_type: &str) -> Result<()> {
    if mime_type.is_empty()
        || !mime_type.is_ascii()
        || mime_type.bytes().any(|byte| byte.is_ascii_control())
        || !mime_type.contains('/')
    {
        return Err(ThumbnailError::InvalidMetadata("invalid MIME type"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_resource_limit_rejects_large_dimensions() {
        let error = ensure_rendered_resource_limits(4097, 4097, 4097 * 4097 * 4).unwrap_err();
        assert!(matches!(
            error,
            ThumbnailError::UnsupportedRenderedThumbnail("rendered PNG resource limit exceeded")
        ));
    }

    #[test]
    fn rgba_buffer_len_rejects_overflow() {
        let error = rgba_buffer_len(u32::MAX, u32::MAX).unwrap_err();
        assert!(matches!(
            error,
            ThumbnailError::UnsupportedRenderedThumbnail("RGBA buffer length overflows usize")
        ));
    }

    #[test]
    fn rendered_png_rejects_apng() {
        let mut output = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut output, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(1, 0).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 0, 0, 255]).unwrap();
        }

        let error = match decode_rendered_png_to_rgba8(&output) {
            Ok(_) => panic!("APNG rendered output was accepted"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            ThumbnailError::UnsupportedRenderedThumbnail("animated PNG output is unsupported")
        ));
    }
}
