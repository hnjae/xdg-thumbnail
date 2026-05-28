// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, CacheRoot, FailureNamespace, OriginalIdentity, ParsedThumbnailPng,
    PersonalThumbnailUri, ReadableOriginalIdentity, UnixMTimeSeconds,
};

#[test]
fn writes_deterministic_failure_namespace_entries() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let namespace = FailureNamespace::new("xdg-thumbnail-0.1.0").unwrap();
    let original = readable_original();

    let first = root.write_failure_entry(&namespace, &original).unwrap();
    let second = root.write_failure_entry(&namespace, &original).unwrap();

    let expected_path = root.personal_path(
        original.identity().uri(),
        &CacheNamespace::Failure(namespace.clone()),
    );
    assert_eq!(first.path(), expected_path.as_path());
    assert_eq!(first.bytes(), second.bytes());
    assert_eq!(std::fs::read(&expected_path).unwrap(), first.bytes());

    let parsed = ParsedThumbnailPng::parse(first.bytes()).unwrap();
    assert_eq!(parsed.width(), 1);
    assert_eq!(parsed.height(), 1);
    assert_eq!(parsed.color_type(), png::ColorType::Rgba);
    assert_eq!(
        parsed.metadata().thumb_uri(),
        Some("file:///home/alice/photo.png")
    );
    assert_eq!(parsed.metadata().thumb_mtime(), Some(42));
    assert_eq!(parsed.metadata().thumb_size(), Some(12));
    assert_eq!(parsed.metadata().thumb_mimetype(), Some("image/png"));
}

fn readable_original() -> ReadableOriginalIdentity {
    ReadableOriginalIdentity::new(
        OriginalIdentity::new(
            PersonalThumbnailUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            UnixMTimeSeconds::new(42),
            Some(12),
            Some("image/png"),
        )
        .unwrap(),
    )
}
