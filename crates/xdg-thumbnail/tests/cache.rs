// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, UNIX_EPOCH};

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, CacheRootProblem, FailureNamespace, PersonalCacheRoot,
    PersonalOriginalIdentity, PersonalOriginalUri, ReadablePersonalOriginalIdentity,
    SharedRepositoryContext, ThumbnailError, ThumbnailSize, UnixMtimeSeconds,
};

#[test]
fn cache_root_uses_absolute_xdg_cache_home_and_home_fallback() {
    let root = PersonalCacheRoot::resolve_from_values(
        Some(OsStr::from_bytes(b"/tmp/cache")),
        Some(OsStr::from_bytes(b"/home/alice")),
    )
    .unwrap();
    assert_eq!(root.as_path(), Path::new("/tmp/cache/thumbnails"));

    let fallback = PersonalCacheRoot::resolve_from_values(
        Some(OsStr::from_bytes(b"relative/cache")),
        Some(OsStr::from_bytes(b"/home/alice")),
    )
    .unwrap();
    assert_eq!(
        fallback.as_path(),
        Path::new("/home/alice/.cache/thumbnails")
    );

    assert!(PersonalCacheRoot::resolve_from_values(None, None).is_err());
}

#[test]
fn explicit_cache_roots_report_typed_absolute_path_errors() {
    let personal_error = PersonalCacheRoot::new(Path::new("relative-cache")).unwrap_err();
    assert!(matches!(
        personal_error,
        ThumbnailError::InvalidCacheRoot {
            ref path,
            problem: CacheRootProblem::NotAbsolute,
            ..
        } if path == Path::new("relative-cache")
    ));

    let shared_error =
        SharedRepositoryContext::new(Path::new("relative-repository"), OsStr::new("photo.png"))
            .unwrap_err();
    assert!(matches!(
        shared_error,
        ThumbnailError::InvalidCacheRoot {
            ref path,
            problem: CacheRootProblem::NotAbsolute,
            ..
        } if path == Path::new("relative-repository")
    ));
}

#[test]
fn local_original_open_errors_carry_the_failing_path() {
    let temp = TempDir::new().unwrap();
    let missing = temp.path().join("missing-original.png");

    let error = ReadablePersonalOriginalIdentity::from_local_path(&missing).unwrap_err();

    assert!(matches!(
        error,
        ThumbnailError::Io {
            context: "open original for reading",
            path: Some(ref path),
            ..
        } if path == &missing
    ));
}

#[test]
fn structured_uri_errors_expose_reason_without_tuple_matching() {
    let error =
        ReadablePersonalOriginalIdentity::from_local_path(Path::new("relative-file")).unwrap_err();

    assert!(matches!(
        error,
        ThumbnailError::InvalidUriIdentity {
            reason: "local path must be absolute",
            ..
        }
    ));
}

#[test]
fn thumbnail_sizes_have_namespace_names_and_limits() {
    assert_eq!(ThumbnailSize::Normal.directory_name(), "normal");
    assert_eq!(ThumbnailSize::Normal.max_dimension(), 128);
    assert_eq!(ThumbnailSize::Large.max_dimension(), 256);
    assert_eq!(ThumbnailSize::XLarge.max_dimension(), 512);
    assert_eq!(ThumbnailSize::XxLarge.max_dimension(), 1024);
}

#[test]
fn namespaces_compute_cache_entry_paths() {
    let root = PersonalCacheRoot::new(Path::new("/tmp/cache/thumbnails")).unwrap();
    let uri = PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap();
    let failure = FailureNamespace::new("xdg-thumbnail+0.1.0").unwrap();

    assert_eq!(
        root.cache_entry_path(&uri, &CacheNamespace::Size(ThumbnailSize::Normal)),
        Path::new("/tmp/cache/thumbnails/normal/82346fd12242a0f50d9cf25786189951.png")
    );
    assert_eq!(
        root.cache_entry_path(&uri, &CacheNamespace::Failure(failure)),
        Path::new(
            "/tmp/cache/thumbnails/fail/xdg-thumbnail+0.1.0/82346fd12242a0f50d9cf25786189951.png"
        )
    );
}

#[test]
fn failure_namespaces_are_direct_ascii_directory_names() {
    assert!(FailureNamespace::new("program-1.0_alpha+build").is_ok());
    let namespace = FailureNamespace::from_str("program-1.0_alpha+build").unwrap();
    assert_eq!(namespace.as_ref(), "program-1.0_alpha+build");

    for invalid in [
        "",
        ".",
        "..",
        "nested/name",
        "has space",
        "snowman☃",
        "bad\nname",
    ] {
        assert!(FailureNamespace::new(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn path_newtypes_expose_unambiguous_path_traits() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = ReadablePersonalOriginalIdentity::assume_readable(
        PersonalOriginalIdentity::new(
            PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            UnixMtimeSeconds::new(42),
        )
        .with_original_byte_size(12),
    );
    let installed = root
        .install_thumbnail_path(&original, ThumbnailSize::Normal, &png_without_metadata())
        .unwrap();

    assert_eq!(root.as_ref(), temp.path().join("thumbnails").as_path());
    assert_eq!(
        installed.as_ref(),
        root.cache_entry_path(
            original.identity().uri(),
            &CacheNamespace::Size(ThumbnailSize::Normal)
        )
        .as_path()
    );
}

#[test]
fn original_identity_preserves_required_freshness_facts() {
    let uri = PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap();
    let mtime = UnixMtimeSeconds::new(42);
    let identity = PersonalOriginalIdentity::new(uri.clone(), mtime)
        .with_original_byte_size(12)
        .with_mime_type("image/png")
        .unwrap();
    let readable = ReadablePersonalOriginalIdentity::assume_readable(identity.clone());

    assert_eq!(identity.uri(), &uri);
    assert_eq!(identity.mtime().as_u64(), 42);
    assert_eq!(identity.original_byte_size(), Some(12));
    assert_eq!(identity.mime_type(), Some("image/png"));
    assert_eq!(readable.identity().uri(), &uri);
}

#[test]
fn original_identity_without_mime_type_needs_no_type_hint() {
    let uri = PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap();
    let identity = PersonalOriginalIdentity::new(uri.clone(), UnixMtimeSeconds::new(42))
        .with_original_byte_size(12);

    assert_eq!(identity.uri(), &uri);
    assert_eq!(identity.mime_type(), None);
}

#[test]
fn unix_mtime_seconds_rejects_pre_epoch_times() {
    assert_eq!(
        UnixMtimeSeconds::from_system_time(UNIX_EPOCH + Duration::from_secs(7))
            .unwrap()
            .as_u64(),
        7
    );
    assert!(UnixMtimeSeconds::from_system_time(UNIX_EPOCH - Duration::from_secs(1)).is_err());
    assert_eq!(
        UnixMtimeSeconds::try_from_i64(7).unwrap(),
        UnixMtimeSeconds::new(7)
    );
    assert!(UnixMtimeSeconds::try_from_i64(-1).is_err());
}

#[test]
fn readable_local_identity_rejects_relative_path_before_opening() {
    let error =
        ReadablePersonalOriginalIdentity::from_local_path(Path::new("relative-file")).unwrap_err();

    assert!(error.to_string().contains("local path must be absolute"));
}

#[test]
fn readable_local_identity_preserves_symlink_path_as_uri_and_stats_target() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target.bin");
    let link = temp.path().join("link.bin");
    std::fs::write(&target, b"abc").unwrap();
    symlink(&target, &link).unwrap();

    let readable = ReadablePersonalOriginalIdentity::from_local_path(&link).unwrap();
    let expected_uri = PersonalOriginalUri::from_absolute_path(&link).unwrap();
    let target_metadata = std::fs::metadata(&target).unwrap();
    let link_metadata = std::fs::symlink_metadata(&link).unwrap();

    assert_eq!(readable.identity().uri(), &expected_uri);
    assert_eq!(
        readable.identity().mtime(),
        UnixMtimeSeconds::from_system_time(target_metadata.modified().unwrap()).unwrap()
    );
    assert_eq!(
        readable.identity().original_byte_size(),
        Some(target_metadata.len())
    );
    assert_ne!(target_metadata.len(), link_metadata.len());
}

#[test]
fn shared_repository_context_computes_contextual_cache_paths() {
    let context =
        SharedRepositoryContext::new(Path::new("/srv/photos"), OsStr::from_bytes(b"picture.png"))
            .unwrap();

    assert_eq!(context.shared_uri().as_str(), "./picture.png");
    assert_eq!(
        context.cache_entry_path(ThumbnailSize::Normal),
        Path::new("/srv/photos/.sh_thumbnails/normal/7fd0e41c1612f860427a76c4100745a3.png")
    );

    assert!(
        SharedRepositoryContext::new(
            Path::new("/srv/photos"),
            OsStr::from_bytes(b"nested/file.png")
        )
        .is_err()
    );
}

fn png_without_metadata() -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 8]).unwrap();
    }
    output
}
