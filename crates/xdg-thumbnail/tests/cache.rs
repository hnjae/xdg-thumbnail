// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, FailureNamespace, OriginalIdentity, PersonalCacheRoot, PersonalOriginalUri,
    ReadableOriginalIdentity, SharedRepositoryContext, ThumbnailSize, UnixMTimeSeconds,
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
fn original_identity_preserves_required_freshness_facts() {
    let uri = PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap();
    let mtime = UnixMTimeSeconds::new(42);
    let identity = OriginalIdentity::new(uri.clone(), mtime)
        .with_original_byte_size(12)
        .with_mime_type("image/png")
        .unwrap();
    let readable = ReadableOriginalIdentity::from_confirmed_readable_identity(identity.clone());

    assert_eq!(identity.uri(), &uri);
    assert_eq!(identity.mtime().as_u64(), 42);
    assert_eq!(identity.original_byte_size(), Some(12));
    assert_eq!(identity.mime_type(), Some("image/png"));
    assert_eq!(readable.identity().uri(), &uri);
}

#[test]
fn original_identity_without_mime_type_needs_no_type_hint() {
    let uri = PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap();
    let identity =
        OriginalIdentity::new(uri.clone(), UnixMTimeSeconds::new(42)).with_original_byte_size(12);

    assert_eq!(identity.uri(), &uri);
    assert_eq!(identity.mime_type(), None);
}

#[test]
fn unix_mtime_seconds_rejects_pre_epoch_times() {
    assert_eq!(
        UnixMTimeSeconds::from_system_time(UNIX_EPOCH + Duration::from_secs(7))
            .unwrap()
            .as_u64(),
        7
    );
    assert!(UnixMTimeSeconds::from_system_time(UNIX_EPOCH - Duration::from_secs(1)).is_err());
    assert_eq!(
        UnixMTimeSeconds::try_from_i64(7).unwrap(),
        UnixMTimeSeconds::new(7)
    );
    assert!(UnixMTimeSeconds::try_from_i64(-1).is_err());
}

#[test]
fn readable_local_identity_rejects_relative_path_before_opening() {
    let error = ReadableOriginalIdentity::from_local_path(Path::new("relative-file")).unwrap_err();

    assert!(error.to_string().contains("local path must be absolute"));
}

#[test]
fn readable_local_identity_preserves_symlink_path_as_uri_and_stats_target() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target.bin");
    let link = temp.path().join("link.bin");
    std::fs::write(&target, b"abc").unwrap();
    symlink(&target, &link).unwrap();

    let readable = ReadableOriginalIdentity::from_local_path(&link).unwrap();
    let expected_uri = PersonalOriginalUri::from_absolute_path(&link).unwrap();
    let target_metadata = std::fs::metadata(&target).unwrap();
    let link_metadata = std::fs::symlink_metadata(&link).unwrap();

    assert_eq!(readable.identity().uri(), &expected_uri);
    assert_eq!(
        readable.identity().mtime(),
        UnixMTimeSeconds::from_system_time(target_metadata.modified().unwrap()).unwrap()
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
        context.thumbnail_path(ThumbnailSize::Normal),
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
