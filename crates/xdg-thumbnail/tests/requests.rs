// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, CacheRoot, FailureEntryWriteRequest, FailureNamespace, OriginalIdentity,
    ParsedThumbnailPng, PersonalThumbnailInspectionRequest, PersonalThumbnailInstallRequest,
    PersonalThumbnailLookupRequest, PersonalThumbnailUri, ReadableOriginalIdentity,
    SharedCacheEntryOutcome, SharedRepositoryContext, SharedThumbnailInspectionRequest,
    SharedThumbnailLookup, SharedThumbnailLookupRequest, ThumbnailLookup, ThumbnailSize,
    UnixMTimeSeconds,
};

#[test]
fn personal_lookup_request_matches_borrowed_lookup() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let request =
        PersonalThumbnailLookupRequest::new(root.clone(), original.clone(), ThumbnailSize::Normal);

    assert_eq!(
        request.validated_path().unwrap(),
        root.validated_personal_path(&original, ThumbnailSize::Normal)
            .unwrap()
    );
    assert_eq!(request.validated_path().unwrap(), ThumbnailLookup::Missing);

    let path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, png_with_metadata(personal_metadata("42"))).unwrap();

    assert_eq!(
        request.validated_path().unwrap(),
        root.validated_personal_path(&original, ThumbnailSize::Normal)
            .unwrap()
    );
    match request.validated_payload().unwrap() {
        ThumbnailLookup::Valid(payload) => {
            assert_eq!(payload.path(), path.as_path());
            assert_eq!(
                payload.metadata().thumb_uri(),
                Some("file:///home/alice/photo.png")
            );
        }
        other => panic!("expected valid personal payload lookup, got {other:?}"),
    }
}

#[test]
fn personal_install_request_matches_borrowed_install_and_normalizes() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let rendered = png_without_metadata(300, 150, png::ColorType::Rgb);
    let request = PersonalThumbnailInstallRequest::new(
        root.clone(),
        original.clone(),
        ThumbnailSize::Normal,
        rendered.clone(),
    );

    let request_install = request.install_payload().unwrap();
    let borrowed_install = root
        .install_personal_thumbnail_payload(&original, ThumbnailSize::Normal, &rendered)
        .unwrap();

    assert_eq!(request_install, borrowed_install);
    assert_eq!(
        std::fs::read(request_install.path()).unwrap(),
        request_install.bytes()
    );

    let parsed = ParsedThumbnailPng::parse(request_install.bytes()).unwrap();
    assert_eq!(parsed.width(), 128);
    assert_eq!(parsed.height(), 64);
    assert_eq!(parsed.color_type(), png::ColorType::Rgba);
    assert_eq!(parsed.metadata().thumb_mtime(), Some(42));
}

#[test]
fn failure_entry_write_request_matches_borrowed_write() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let namespace = FailureNamespace::new("xdg-thumbnail-0.1.0").unwrap();
    let original = readable_original();
    let request = FailureEntryWriteRequest::new(root.clone(), namespace.clone(), original.clone());

    let request_payload = request.write_payload().unwrap();
    let borrowed_payload = root
        .write_failure_entry_payload(&namespace, &original)
        .unwrap();
    assert_eq!(request_payload, borrowed_payload);

    let request_path = request.write_path().unwrap();
    let borrowed_path = root
        .write_failure_entry_path(&namespace, &original)
        .unwrap();
    assert_eq!(request_path, borrowed_path);

    let expected_path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Failure(namespace),
    );
    assert_eq!(request_payload.path(), expected_path.as_path());
    assert_eq!(
        std::fs::read(expected_path).unwrap(),
        request_payload.bytes()
    );
}

#[test]
fn personal_inspection_request_owns_size_vector() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let installed = root
        .install_personal_thumbnail_payload(
            &original,
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1, png::ColorType::Rgba),
        )
        .unwrap();
    let sizes = vec![ThumbnailSize::Normal];
    let request = PersonalThumbnailInspectionRequest::new(root, sizes, false);

    let entries = request.inspect().unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path(), installed.path());
    assert_eq!(
        entries[0].namespace(),
        &CacheNamespace::Size(ThumbnailSize::Normal)
    );
}

#[test]
fn shared_lookup_and_inspection_requests_match_borrowed_api() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let path = context.thumbnail_path(ThumbnailSize::Normal);
    let lookup_request = SharedThumbnailLookupRequest::new(
        context.clone(),
        Some(UnixMTimeSeconds::new(42)),
        Some(12),
        ThumbnailSize::Normal,
    );

    assert_eq!(
        lookup_request.validated_path().unwrap(),
        context
            .validated_thumbnail_path(
                Some(UnixMTimeSeconds::new(42)),
                Some(12),
                ThumbnailSize::Normal,
            )
            .unwrap()
    );
    assert_eq!(
        lookup_request.validated_path().unwrap(),
        SharedThumbnailLookup::Missing
    );

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let shared_bytes = shared_png(shared_metadata("./picture.png", Some("42"), Some("12")));
    std::fs::write(&path, &shared_bytes).unwrap();

    assert_eq!(
        lookup_request.validated_payload().unwrap(),
        context
            .validated_thumbnail_payload(
                Some(UnixMTimeSeconds::new(42)),
                Some(12),
                ThumbnailSize::Normal,
            )
            .unwrap()
    );

    let inspection_request = SharedThumbnailInspectionRequest::new(
        context,
        vec![ThumbnailSize::Normal],
        Some(UnixMTimeSeconds::new(42)),
        Some(12),
    );
    let inspections = inspection_request.inspect().unwrap();
    assert_eq!(inspections.len(), 1);
    assert_eq!(
        inspections[0].outcome(),
        &SharedCacheEntryOutcome::FullyVerified
    );
    assert_eq!(inspections[0].path(), path.as_path());
}

fn readable_original() -> ReadableOriginalIdentity {
    ReadableOriginalIdentity::new(
        OriginalIdentity::with_mime_type(
            PersonalThumbnailUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            UnixMTimeSeconds::new(42),
            Some(12),
            "image/png",
        )
        .unwrap(),
    )
}

fn personal_metadata(mtime: &'static str) -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("Thumb::URI", "file:///home/alice/photo.png"),
        ("Thumb::MTime", mtime),
        ("Thumb::Size", "12"),
        ("Thumb::Mimetype", "image/png"),
    ])
}

fn shared_metadata(
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

fn png_without_metadata(width: u32, height: u32, color_type: png::ColorType) -> Vec<u8> {
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
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
    }
    output
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

fn shared_png(metadata: BTreeMap<&str, &str>) -> Vec<u8> {
    png_with_metadata(metadata)
}
