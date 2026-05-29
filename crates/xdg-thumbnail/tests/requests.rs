// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryInspection, CacheNamespace, FailureEntryWriteRequest, FailureNamespace,
    InstalledThumbnailBytes, InstalledThumbnailPath, OriginalIdentity, OwnedRawThumbnailImage,
    ParsedThumbnailPng, PersonalCacheRoot, PersonalOriginalUri, PersonalThumbnailInspectionRequest,
    PersonalThumbnailInstallRequest, PersonalThumbnailLookup, PersonalThumbnailLookupRequest,
    PersonalThumbnailRawInstallRequest, RawThumbnailImage, RawThumbnailPixelFormat,
    ReadableOriginalIdentity, SharedCacheEntryInspection, SharedCacheEntryOutcome,
    SharedRepositoryContext, SharedThumbnailInspectionRequest, SharedThumbnailLookup,
    SharedThumbnailLookupRequest, SharedThumbnailMetadataPolicy, ThumbnailBytesLookupEntry,
    ThumbnailPathLookupEntry, ThumbnailPngColorType, ThumbnailSize, UnixMTimeSeconds,
};

#[test]
fn owned_request_and_result_types_are_send_sync_static() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}

    assert_send_sync_static::<PersonalThumbnailLookupRequest>();
    assert_send_sync_static::<PersonalThumbnailInstallRequest>();
    assert_send_sync_static::<PersonalThumbnailRawInstallRequest>();
    assert_send_sync_static::<FailureEntryWriteRequest>();
    assert_send_sync_static::<PersonalThumbnailInspectionRequest>();
    assert_send_sync_static::<SharedThumbnailLookupRequest>();
    assert_send_sync_static::<SharedThumbnailInspectionRequest>();
    assert_send_sync_static::<SharedThumbnailMetadataPolicy>();
    assert_send_sync_static::<ThumbnailPathLookupEntry>();
    assert_send_sync_static::<ThumbnailBytesLookupEntry>();
    assert_send_sync_static::<InstalledThumbnailPath>();
    assert_send_sync_static::<InstalledThumbnailBytes>();
    assert_send_sync_static::<CacheEntryInspection>();
    assert_send_sync_static::<SharedCacheEntryInspection>();
    assert_send_sync_static::<PersonalThumbnailLookup<ThumbnailBytesLookupEntry>>();
    assert_send_sync_static::<SharedThumbnailLookup<ThumbnailBytesLookupEntry>>();
}

fn run_blocking_style<F, R>(operation: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    operation()
}

#[test]
fn personal_lookup_request_matches_borrowed_lookup() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let request =
        PersonalThumbnailLookupRequest::new(root.clone(), original.clone(), ThumbnailSize::Normal);

    assert_eq!(
        request.clone().validated_path().unwrap(),
        root.validated_personal_path(&original, ThumbnailSize::Normal)
            .unwrap()
    );
    assert_eq!(
        run_blocking_style(move || request.validated_path()).unwrap(),
        PersonalThumbnailLookup::Missing
    );

    let path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, png_with_metadata(personal_metadata("42"))).unwrap();
    let request =
        PersonalThumbnailLookupRequest::new(root.clone(), original.clone(), ThumbnailSize::Normal);

    assert_eq!(
        request.clone().validated_path().unwrap(),
        root.validated_personal_path(&original, ThumbnailSize::Normal)
            .unwrap()
    );
    match request.clone().validated_path().unwrap() {
        PersonalThumbnailLookup::Valid(valid_path) => {
            let (owned_path, metadata) = valid_path.into_parts();
            assert_eq!(owned_path, path);
            assert_eq!(metadata.thumb_size(), Some(12));
        }
        other => panic!("expected valid personal path lookup, got {other:?}"),
    }
    match request.validated_bytes().unwrap() {
        PersonalThumbnailLookup::Valid(bytes) => {
            assert_eq!(bytes.path(), path.as_path());
            assert_eq!(
                bytes.metadata().thumb_uri(),
                Some("file:///home/alice/photo.png")
            );
            let (owned_path, owned_bytes, metadata) = bytes.into_parts();
            assert_eq!(owned_path, path);
            assert_eq!(owned_bytes, png_with_metadata(personal_metadata("42")));
            assert_eq!(metadata.thumb_mtime(), Some(UnixMTimeSeconds::new(42)));
        }
        other => panic!("expected valid personal bytes lookup, got {other:?}"),
    }
}

#[test]
fn personal_install_request_matches_borrowed_install_and_normalizes() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let rendered = png_without_metadata(300, 150, png::ColorType::Rgb);
    let request = PersonalThumbnailInstallRequest::new(
        root.clone(),
        original.clone(),
        ThumbnailSize::Normal,
        rendered.clone(),
    );

    let request_install = run_blocking_style(move || request.install_bytes()).unwrap();
    let borrowed_install = root
        .install_personal_thumbnail_bytes(&original, ThumbnailSize::Normal, &rendered)
        .unwrap();

    assert_eq!(request_install, borrowed_install);
    assert_eq!(
        std::fs::read(request_install.path()).unwrap(),
        request_install.bytes()
    );
    let (installed_path, installed_bytes) = request_install.clone().into_parts();
    assert_eq!(installed_path, request_install.path());
    assert_eq!(installed_bytes, request_install.bytes());

    let parsed = ParsedThumbnailPng::parse(request_install.bytes()).unwrap();
    assert_eq!(parsed.width(), 128);
    assert_eq!(parsed.height(), 64);
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
    assert_eq!(
        parsed.metadata().thumb_mtime(),
        Some(UnixMTimeSeconds::new(42))
    );
}

#[test]
fn personal_raw_install_request_matches_borrowed_install_and_normalizes() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let width = 300;
    let height = 150;
    let stride = width * 3;
    let pixels = vec![64; stride as usize * height as usize];
    let owned_image = OwnedRawThumbnailImage::new(
        width,
        height,
        stride as usize,
        RawThumbnailPixelFormat::Rgb8,
        pixels.clone(),
    )
    .unwrap();
    assert_eq!(owned_image.width(), width);
    assert_eq!(owned_image.height(), height);
    assert_eq!(owned_image.stride(), stride as usize);
    assert_eq!(owned_image.format(), RawThumbnailPixelFormat::Rgb8);
    assert_eq!(owned_image.pixels(), pixels.as_slice());
    assert_eq!(
        owned_image.clone().into_parts(),
        (
            width,
            height,
            stride as usize,
            RawThumbnailPixelFormat::Rgb8,
            pixels.clone()
        )
    );
    let request = PersonalThumbnailRawInstallRequest::new(
        root.clone(),
        original.clone(),
        ThumbnailSize::Normal,
        owned_image,
    );

    let request_install = run_blocking_style(move || request.install_bytes()).unwrap();
    let borrowed_image = RawThumbnailImage::new(
        width,
        height,
        stride as usize,
        RawThumbnailPixelFormat::Rgb8,
        &pixels,
    )
    .unwrap();
    let borrowed_install = root
        .install_personal_thumbnail_raw_bytes(&original, ThumbnailSize::Normal, borrowed_image)
        .unwrap();

    assert_eq!(request_install, borrowed_install);
    assert_eq!(
        std::fs::read(request_install.path()).unwrap(),
        request_install.bytes()
    );

    let path_request = PersonalThumbnailRawInstallRequest::new(
        root.clone(),
        original.clone(),
        ThumbnailSize::Normal,
        OwnedRawThumbnailImage::new(
            width,
            height,
            stride as usize,
            RawThumbnailPixelFormat::Rgb8,
            pixels.clone(),
        )
        .unwrap(),
    );
    let request_path = run_blocking_style(move || path_request.install_path()).unwrap();
    assert_eq!(request_path.path(), request_install.path());
    assert_eq!(request_path.clone().into_path_buf(), request_install.path());

    let parsed = ParsedThumbnailPng::parse(request_install.bytes()).unwrap();
    assert_eq!(parsed.width(), 128);
    assert_eq!(parsed.height(), 64);
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
    assert_eq!(
        parsed.metadata().thumb_mtime(),
        Some(UnixMTimeSeconds::new(42))
    );

    let parts_image =
        OwnedRawThumbnailImage::new(1, 1, 4, RawThumbnailPixelFormat::Rgba8, vec![1, 2, 3, 4])
            .unwrap();
    let parts_request = PersonalThumbnailRawInstallRequest::new(
        root.clone(),
        original.clone(),
        ThumbnailSize::Large,
        parts_image,
    );
    let (parts_root, parts_original, parts_size, parts_image) = parts_request.into_parts();
    assert_eq!(parts_root, root);
    assert_eq!(parts_original, original);
    assert_eq!(parts_size, ThumbnailSize::Large);
    let installed_from_parts = parts_root
        .install_personal_thumbnail_raw_bytes(
            &parts_original,
            parts_size,
            parts_image.as_borrowed(),
        )
        .unwrap();
    assert!(installed_from_parts.path().exists());
}

#[test]
fn failure_entry_write_request_matches_borrowed_write() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let namespace = FailureNamespace::new("xdg-thumbnail-0.1.0").unwrap();
    let original = readable_original();
    let request = FailureEntryWriteRequest::new(root.clone(), namespace.clone(), original.clone());

    let request_bytes = request.clone().write_bytes().unwrap();
    let borrowed_bytes = root
        .write_failure_entry_bytes(&namespace, &original)
        .unwrap();
    assert_eq!(request_bytes, borrowed_bytes);

    let request_path = run_blocking_style(move || request.write_path()).unwrap();
    let borrowed_path = root
        .write_failure_entry_path(&namespace, &original)
        .unwrap();
    assert_eq!(request_path, borrowed_path);

    let expected_path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Failure(namespace),
    );
    assert_eq!(request_bytes.path(), expected_path.as_path());
    assert_eq!(std::fs::read(expected_path).unwrap(), request_bytes.bytes());
}

#[test]
fn personal_inspection_request_owns_size_vector() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let installed = root
        .install_personal_thumbnail_bytes(
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
        ThumbnailSize::Normal,
        SharedThumbnailMetadataPolicy::RequireComplete,
        Some(UnixMTimeSeconds::new(42)),
        Some(12),
    );

    assert_eq!(
        lookup_request.clone().lookup_path().unwrap(),
        context
            .lookup_thumbnail_path(
                ThumbnailSize::Normal,
                SharedThumbnailMetadataPolicy::RequireComplete,
                Some(UnixMTimeSeconds::new(42)),
                Some(12),
            )
            .unwrap()
    );
    assert_eq!(
        run_blocking_style(move || lookup_request.lookup_path()).unwrap(),
        SharedThumbnailLookup::Missing
    );

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let shared_bytes = shared_png(shared_metadata("./picture.png", Some("42"), Some("12")));
    std::fs::write(&path, &shared_bytes).unwrap();

    let lookup_request = SharedThumbnailLookupRequest::new(
        context.clone(),
        ThumbnailSize::Normal,
        SharedThumbnailMetadataPolicy::RequireComplete,
        Some(UnixMTimeSeconds::new(42)),
        Some(12),
    );
    assert_eq!(
        run_blocking_style(move || lookup_request.lookup_bytes()).unwrap(),
        context
            .lookup_thumbnail_bytes(
                ThumbnailSize::Normal,
                SharedThumbnailMetadataPolicy::RequireComplete,
                Some(UnixMTimeSeconds::new(42)),
                Some(12),
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
    ReadableOriginalIdentity::from_confirmed_readable_identity(
        OriginalIdentity::with_mime_type(
            PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
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
