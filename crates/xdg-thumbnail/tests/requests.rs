// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryInspection, CacheEntryInspectionOutcome, CacheEntryInspectionParts, CacheNamespace,
    FailureEntryInspectionRequest, FailureEntryInspectionRequestParts, FailureEntryWriteRequest,
    FailureEntryWriteRequestParts, FailureNamespace, InstalledThumbnailPath,
    InstalledThumbnailPngBytes, InstalledThumbnailPngBytesParts, NonstandardEntryPolicy,
    OriginalUriIdentity, OwnedRawThumbnailImage, OwnedRawThumbnailImageParts, ParsedThumbnailPng,
    PersonalCacheRoot, PersonalOriginalIdentity, PersonalOriginalUri,
    PersonalThumbnailInspectionRequest, PersonalThumbnailInspectionRequestParts,
    PersonalThumbnailInstallRequest, PersonalThumbnailInstallRequestParts, PersonalThumbnailLookup,
    PersonalThumbnailLookupRequest, PersonalThumbnailLookupRequestParts,
    PersonalThumbnailRawInstallRequest, PersonalThumbnailRawInstallRequestParts, RawThumbnailImage,
    RawThumbnailPixelFormat, ReadablePersonalOriginalIdentity, SharedCacheEntryInspection,
    SharedCacheEntryInspectionParts, SharedCacheEntryOutcome, SharedOriginalFacts,
    SharedOriginalMetadata, SharedRepositoryContext, SharedThumbnailInspectionRequest,
    SharedThumbnailInspectionRequestParts, SharedThumbnailLookup, SharedThumbnailLookupRequest,
    SharedThumbnailLookupRequestParts, SharedThumbnailMetadataPolicy, ThumbnailMetadataKey,
    ThumbnailMetadataProblem, ThumbnailMetadataProblemKind, ThumbnailPathLookupEntry,
    ThumbnailPathLookupEntryParts, ThumbnailPngBytesLookupEntry, ThumbnailPngBytesLookupEntryParts,
    ThumbnailPngColorType, ThumbnailRgba8LookupEntry, ThumbnailRgba8LookupEntryParts,
    ThumbnailSize, UnixMtimeSeconds,
};

#[test]
fn owned_request_and_result_types_are_send_sync_static() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}

    assert_send_sync_static::<PersonalThumbnailLookupRequest>();
    assert_send_sync_static::<PersonalThumbnailLookupRequestParts>();
    assert_send_sync_static::<PersonalThumbnailInstallRequest>();
    assert_send_sync_static::<PersonalThumbnailInstallRequestParts>();
    assert_send_sync_static::<PersonalThumbnailRawInstallRequest>();
    assert_send_sync_static::<PersonalThumbnailRawInstallRequestParts>();
    assert_send_sync_static::<FailureEntryWriteRequest>();
    assert_send_sync_static::<FailureEntryWriteRequestParts>();
    assert_send_sync_static::<FailureEntryInspectionRequest>();
    assert_send_sync_static::<FailureEntryInspectionRequestParts>();
    assert_send_sync_static::<PersonalThumbnailInspectionRequest>();
    assert_send_sync_static::<PersonalThumbnailInspectionRequestParts>();
    assert_send_sync_static::<SharedThumbnailLookupRequest>();
    assert_send_sync_static::<SharedThumbnailLookupRequestParts>();
    assert_send_sync_static::<SharedThumbnailInspectionRequest>();
    assert_send_sync_static::<SharedThumbnailInspectionRequestParts>();
    assert_send_sync_static::<SharedThumbnailMetadataPolicy>();
    assert_send_sync_static::<SharedOriginalFacts>();
    assert_send_sync_static::<SharedOriginalMetadata>();
    assert_send_sync_static::<ThumbnailMetadataKey>();
    assert_send_sync_static::<ThumbnailMetadataProblem>();
    assert_send_sync_static::<ThumbnailMetadataProblemKind>();
    assert_send_sync_static::<NonstandardEntryPolicy>();
    assert_send_sync_static::<ThumbnailPathLookupEntry>();
    assert_send_sync_static::<ThumbnailPathLookupEntryParts>();
    assert_send_sync_static::<ThumbnailPngBytesLookupEntry>();
    assert_send_sync_static::<ThumbnailPngBytesLookupEntryParts>();
    assert_send_sync_static::<ThumbnailRgba8LookupEntry>();
    assert_send_sync_static::<ThumbnailRgba8LookupEntryParts>();
    assert_send_sync_static::<InstalledThumbnailPath>();
    assert_send_sync_static::<InstalledThumbnailPngBytes>();
    assert_send_sync_static::<InstalledThumbnailPngBytesParts>();
    assert_send_sync_static::<OwnedRawThumbnailImageParts>();
    assert_send_sync_static::<CacheEntryInspection>();
    assert_send_sync_static::<CacheEntryInspectionParts>();
    assert_send_sync_static::<SharedCacheEntryInspection>();
    assert_send_sync_static::<SharedCacheEntryInspectionParts>();
    assert_send_sync_static::<PersonalThumbnailLookup<ThumbnailPngBytesLookupEntry>>();
    assert_send_sync_static::<SharedThumbnailLookup<ThumbnailPngBytesLookupEntry>>();
    assert_send_sync_static::<PersonalThumbnailLookup<ThumbnailRgba8LookupEntry>>();
    assert_send_sync_static::<SharedThumbnailLookup<ThumbnailRgba8LookupEntry>>();
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
        request.clone().lookup_path().unwrap(),
        root.lookup_thumbnail_path(&original, ThumbnailSize::Normal)
            .unwrap()
    );
    assert_eq!(
        run_blocking_style(move || request.lookup_path()).unwrap(),
        PersonalThumbnailLookup::Missing
    );

    let path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Size(ThumbnailSize::Normal),
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, png_with_metadata(personal_metadata("42"))).unwrap();
    let request =
        PersonalThumbnailLookupRequest::new(root.clone(), original.clone(), ThumbnailSize::Normal);

    assert_eq!(
        request.clone().lookup_path().unwrap(),
        root.lookup_thumbnail_path(&original, ThumbnailSize::Normal)
            .unwrap()
    );
    match request.clone().lookup_path().unwrap() {
        PersonalThumbnailLookup::Valid(valid_path) => {
            let parts = valid_path.into_parts();
            assert_eq!(parts.path, path);
            assert_eq!(parts.metadata.thumb_size(), Some(12));
        }
        other => panic!("expected valid personal path lookup, got {other:?}"),
    }
    match request.lookup_png_bytes().unwrap() {
        PersonalThumbnailLookup::Valid(bytes) => {
            assert_eq!(bytes.path(), path.as_path());
            assert_eq!(
                bytes.metadata().thumb_uri(),
                Some("file:///home/alice/photo.png")
            );
            let parts = bytes.into_parts();
            assert_eq!(parts.path, path);
            assert_eq!(parts.png_bytes, png_with_metadata(personal_metadata("42")));
            assert_eq!(
                parts.metadata.thumb_mtime(),
                Some(UnixMtimeSeconds::new(42))
            );
        }
        other => panic!("expected valid personal bytes lookup, got {other:?}"),
    }

    let request =
        PersonalThumbnailLookupRequest::new(root.clone(), original.clone(), ThumbnailSize::Normal);
    assert_eq!(
        request.clone().lookup_rgba8().unwrap(),
        root.lookup_thumbnail_rgba8(&original, ThumbnailSize::Normal)
            .unwrap()
    );
    match request.lookup_rgba8().unwrap() {
        PersonalThumbnailLookup::Valid(rgba8) => {
            assert_eq!(rgba8.path(), path.as_path());
            assert_eq!((rgba8.width(), rgba8.height(), rgba8.stride()), (2, 1, 8));
            assert_eq!(rgba8.pixels(), &[255; 8]);
            assert_eq!(rgba8.metadata().thumb_size(), Some(12));
        }
        other => panic!("expected valid personal RGBA8 lookup, got {other:?}"),
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

    let request_install = run_blocking_style(move || request.install_png_bytes()).unwrap();
    let borrowed_install = root
        .install_thumbnail_png_bytes(&original, ThumbnailSize::Normal, &rendered)
        .unwrap();

    assert_eq!(request_install, borrowed_install);
    assert_eq!(
        std::fs::read(request_install.path()).unwrap(),
        request_install.png_bytes()
    );
    let expected_path = request_install.path().to_owned();
    let expected_bytes = request_install.png_bytes().to_vec();
    let installed_parts = request_install.into_parts();
    assert_eq!(installed_parts.path, expected_path);
    assert_eq!(installed_parts.png_bytes, expected_bytes);

    let parsed = ParsedThumbnailPng::parse(&installed_parts.png_bytes).unwrap();
    assert_eq!(parsed.width(), 128);
    assert_eq!(parsed.height(), 64);
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
    assert_eq!(
        parsed.metadata().thumb_mtime(),
        Some(UnixMtimeSeconds::new(42))
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
    let owned_image_for_parts = OwnedRawThumbnailImage::new(
        width,
        height,
        stride as usize,
        RawThumbnailPixelFormat::Rgb8,
        pixels.clone(),
    )
    .unwrap();
    let image_parts = owned_image_for_parts.into_parts();
    assert_eq!(image_parts.width, width);
    assert_eq!(image_parts.height, height);
    assert_eq!(image_parts.stride, stride as usize);
    assert_eq!(image_parts.format, RawThumbnailPixelFormat::Rgb8);
    assert_eq!(image_parts.pixels, pixels.clone());
    let request = PersonalThumbnailRawInstallRequest::new(
        root.clone(),
        original.clone(),
        ThumbnailSize::Normal,
        owned_image,
    );

    let request_install = run_blocking_style(move || request.install_png_bytes()).unwrap();
    let borrowed_image = RawThumbnailImage::new(
        width,
        height,
        stride as usize,
        RawThumbnailPixelFormat::Rgb8,
        &pixels,
    )
    .unwrap();
    let borrowed_install = root
        .install_thumbnail_raw_png_bytes(&original, ThumbnailSize::Normal, borrowed_image)
        .unwrap();

    assert_eq!(request_install, borrowed_install);
    assert_eq!(
        std::fs::read(request_install.path()).unwrap(),
        request_install.png_bytes()
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

    let parsed = ParsedThumbnailPng::parse(request_install.png_bytes()).unwrap();
    assert_eq!(parsed.width(), 128);
    assert_eq!(parsed.height(), 64);
    assert_eq!(parsed.color_type(), ThumbnailPngColorType::Rgba);
    assert_eq!(
        parsed.metadata().thumb_mtime(),
        Some(UnixMtimeSeconds::new(42))
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
    let parts = parts_request.into_parts();
    assert_eq!(parts.root, root);
    assert_eq!(parts.original, original);
    assert_eq!(parts.size, ThumbnailSize::Large);
    let installed_from_parts = parts
        .root
        .install_thumbnail_raw_png_bytes(&parts.original, parts.size, parts.image.as_borrowed())
        .unwrap();
    assert!(installed_from_parts.path().exists());
}

#[test]
fn failure_entry_write_request_matches_borrowed_write() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let namespace = FailureNamespace::new("xdg-thumbnail-0.1.0").unwrap();
    let original = readable_original();
    let request = FailureEntryWriteRequest::new(root.clone(), original.clone(), namespace.clone());

    let request_bytes = request.clone().write_png_bytes().unwrap();
    let borrowed_bytes = root
        .write_failure_entry_png_bytes(&original, &namespace)
        .unwrap();
    assert_eq!(request_bytes, borrowed_bytes);

    let request_path = run_blocking_style(move || request.write_path()).unwrap();
    let borrowed_path = root
        .write_failure_entry_path(&original, &namespace)
        .unwrap();
    assert_eq!(request_path, borrowed_path);

    let expected_path = root.cache_entry_path(
        original.identity().uri(),
        &CacheNamespace::Failure(namespace),
    );
    assert_eq!(request_bytes.path(), expected_path.as_path());
    assert_eq!(
        std::fs::read(expected_path).unwrap(),
        request_bytes.png_bytes()
    );
}

#[test]
fn failure_entry_inspection_request_matches_borrowed_inspection() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let namespace = FailureNamespace::new("xdg-thumbnail-0.1.0").unwrap();
    let original = readable_original();
    root.write_failure_entry_png_bytes(&original, &namespace)
        .unwrap();
    let request = FailureEntryInspectionRequest::new(root.clone(), NonstandardEntryPolicy::Exclude);

    let parts = request.clone().into_parts();
    assert_eq!(parts.root, root);
    assert_eq!(
        parts.nonstandard_entry_policy,
        NonstandardEntryPolicy::Exclude
    );

    assert_eq!(
        request.clone().inspect().unwrap(),
        root.inspect_failure_entries(NonstandardEntryPolicy::Exclude)
            .unwrap()
    );

    let entries = run_blocking_style(move || request.inspect()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].namespace(),
        &CacheNamespace::Failure(namespace.clone())
    );
}

#[test]
fn personal_inspection_request_owns_size_vector() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let installed = root
        .install_thumbnail_png_bytes(
            &original,
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1, png::ColorType::Rgba),
        )
        .unwrap();
    let sizes = vec![ThumbnailSize::Normal];
    let request =
        PersonalThumbnailInspectionRequest::new(root, sizes, NonstandardEntryPolicy::Exclude);
    let parts = request.clone().into_parts();
    assert_eq!(
        parts.nonstandard_entry_policy,
        NonstandardEntryPolicy::Exclude
    );

    let mut entries = request.inspect().unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path(), installed.path());
    assert_eq!(
        entries[0].namespace(),
        &CacheNamespace::Size(ThumbnailSize::Normal)
    );
    let parts = entries.pop().unwrap().into_parts();
    assert_eq!(parts.outcome, CacheEntryInspectionOutcome::Unvalidated);
    match parts.original_uri {
        Some(OriginalUriIdentity::Personal(uri)) => {
            assert_eq!(uri, original.identity().uri().clone());
        }
        other => panic!("expected personal original URI, got {other:?}"),
    }
    assert!(parts.timestamps.modified_at().is_some());
    assert_eq!(parts.namespace, CacheNamespace::Size(ThumbnailSize::Normal));
    assert_eq!(parts.path, installed.path());
    assert_eq!(parts.handle.path(), installed.path());
}

#[test]
fn shared_lookup_and_inspection_requests_match_borrowed_api() {
    let temp = TempDir::new().unwrap();
    let context =
        SharedRepositoryContext::new(temp.path(), OsStr::from_bytes(b"picture.png")).unwrap();
    let path = context.cache_entry_path(ThumbnailSize::Normal);
    let original_metadata = SharedOriginalMetadata::new()
        .with_mtime(UnixMtimeSeconds::new(42))
        .with_original_byte_size(12);
    let require_complete = SharedOriginalFacts::new(
        SharedThumbnailMetadataPolicy::RequireComplete,
        original_metadata,
    );
    assert_eq!(
        require_complete.metadata_policy(),
        SharedThumbnailMetadataPolicy::RequireComplete
    );
    assert_eq!(require_complete.mtime(), Some(UnixMtimeSeconds::new(42)));
    assert_eq!(require_complete.original_byte_size(), Some(12));
    assert_eq!(require_complete.metadata(), original_metadata);

    let lookup_request =
        SharedThumbnailLookupRequest::new(context.clone(), require_complete, ThumbnailSize::Normal);
    let parts = lookup_request.clone().into_parts();
    assert_eq!(parts.context, context);
    assert_eq!(parts.original_facts, require_complete);
    assert_eq!(parts.size, ThumbnailSize::Normal);

    assert_eq!(
        lookup_request.clone().lookup_path().unwrap(),
        context
            .lookup_thumbnail_path(require_complete, ThumbnailSize::Normal)
            .unwrap()
    );
    assert_eq!(
        run_blocking_style(move || lookup_request.lookup_path()).unwrap(),
        SharedThumbnailLookup::Missing
    );

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let shared_bytes = shared_png(shared_metadata("./picture.png", Some("42"), Some("12")));
    std::fs::write(&path, &shared_bytes).unwrap();

    let lookup_request =
        SharedThumbnailLookupRequest::new(context.clone(), require_complete, ThumbnailSize::Normal);
    assert_eq!(
        run_blocking_style(move || lookup_request.lookup_png_bytes()).unwrap(),
        context
            .lookup_thumbnail_png_bytes(require_complete, ThumbnailSize::Normal)
            .unwrap()
    );
    let lookup_request =
        SharedThumbnailLookupRequest::new(context.clone(), require_complete, ThumbnailSize::Normal);
    assert_eq!(
        run_blocking_style(move || lookup_request.lookup_rgba8()).unwrap(),
        context
            .lookup_thumbnail_rgba8(require_complete, ThumbnailSize::Normal)
            .unwrap()
    );

    let inspection_request = SharedThumbnailInspectionRequest::new(
        context.clone(),
        vec![ThumbnailSize::Normal],
        original_metadata,
    );
    let mut inspections = inspection_request.inspect().unwrap();
    assert_eq!(inspections.len(), 1);
    assert_eq!(
        inspections[0].outcome(),
        &SharedCacheEntryOutcome::FullyVerified
    );
    assert_eq!(inspections[0].path(), path.as_path());
    let parts = inspections.pop().unwrap().into_parts();
    assert_eq!(parts.outcome, SharedCacheEntryOutcome::FullyVerified);
    assert_eq!(parts.shared_uri, context.shared_uri().clone());
    assert!(parts.timestamps.modified_at().is_some());
    assert_eq!(parts.size, ThumbnailSize::Normal);
    assert_eq!(parts.path, path);
    assert_eq!(parts.metadata.unwrap().thumb_size(), Some(12));
}

fn readable_original() -> ReadablePersonalOriginalIdentity {
    ReadablePersonalOriginalIdentity::from_confirmed_readable_identity(
        PersonalOriginalIdentity::new(
            PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            UnixMtimeSeconds::new(42),
        )
        .with_original_byte_size(12)
        .with_mime_type("image/png")
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
