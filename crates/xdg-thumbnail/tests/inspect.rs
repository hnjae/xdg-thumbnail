// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    CacheEntryProblem, CacheNamespace, CacheRoot, FailureNamespace, OriginalIdentity,
    PersonalThumbnailUri, ReadableOriginalIdentity, ThumbnailSize, ThumbnailUriIdentity,
    ValidationOutcome,
};

#[test]
fn inspection_iterates_standard_entries_and_reports_facts() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let installed = root
        .install_personal_thumbnail(
            &original,
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1),
        )
        .unwrap();
    let nonstandard_dir = root.as_path().join("normal");
    std::fs::write(nonstandard_dir.join("note.txt"), b"not a thumbnail").unwrap();

    let default_entries = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], false)
        .unwrap();
    assert_eq!(default_entries.len(), 1);
    assert_eq!(default_entries[0].path(), installed.path());
    assert_eq!(
        default_entries[0].namespace(),
        &CacheNamespace::Size(ThumbnailSize::Normal)
    );
    assert_eq!(
        default_entries[0].outcome(),
        &ValidationOutcome::UncheckedInspection
    );
    assert!(default_entries[0].timestamps().modified_at().is_some());
    assert!(matches!(
        default_entries[0].original_uri(),
        Some(ThumbnailUriIdentity::Personal(uri)) if uri.as_str() == "file:///home/alice/photo.png"
    ));

    let visible_entries = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], true)
        .unwrap();
    assert_eq!(visible_entries.len(), 2);
    assert!(visible_entries.iter().any(|entry| {
        matches!(entry.outcome(), ValidationOutcome::Invalid(problems) if problems.contains(&CacheEntryProblem::NonstandardFilename))
    }));
}

#[test]
fn failure_iteration_is_limited_to_one_real_namespace_level() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let namespace = FailureNamespace::new("app-1").unwrap();
    root.write_failure_entry(&namespace, &original).unwrap();

    let nested = root.as_path().join("fail/app-1/nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("abcdefabcdefabcdefabcdefabcdefab.png"),
        b"nested",
    )
    .unwrap();

    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, root.as_path().join("fail/link")).unwrap();

    let entries = root.inspect_failure_entries(false).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].namespace(),
        &CacheNamespace::Failure(FailureNamespace::new("app-1").unwrap())
    );
}

#[test]
fn inspection_does_not_follow_symlinked_size_namespace_directories() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let outside = temp.path().join("outside-normal");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(root.as_path()).unwrap();
    std::fs::write(
        outside.join("abcdefabcdefabcdefabcdefabcdefab.png"),
        b"outside",
    )
    .unwrap();
    symlink(&outside, root.as_path().join("normal")).unwrap();

    let entries = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], false)
        .unwrap();

    assert!(entries.is_empty());
}

#[test]
fn cache_entry_handles_remove_files_without_following_symlinks() {
    let temp = TempDir::new().unwrap();
    let root = CacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let installed = root
        .install_personal_thumbnail(
            &original,
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1),
        )
        .unwrap();
    let handle = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], false)
        .unwrap()[0]
        .handle()
        .clone();
    handle.remove().unwrap();
    assert!(!installed.path().exists());

    let namespace_dir = root.as_path().join("normal");
    std::fs::create_dir_all(&namespace_dir).unwrap();
    let outside = temp.path().join("outside.png");
    std::fs::write(&outside, b"outside").unwrap();
    let symlink_path = namespace_dir.join("abcdefabcdefabcdefabcdefabcdefab.png");
    symlink(&outside, &symlink_path).unwrap();

    let symlink_handle = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], false)
        .unwrap()[0]
        .handle()
        .clone();
    assert!(symlink_handle.remove().is_err());
    assert!(outside.exists());
}

fn readable_original() -> ReadableOriginalIdentity {
    ReadableOriginalIdentity::new(
        OriginalIdentity::new(
            PersonalThumbnailUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            xdg_thumbnail::UnixMTimeSeconds::new(42),
            Some(12),
            Some("image/png"),
        )
        .unwrap(),
    )
}

fn png_without_metadata(width: u32, height: u32) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        for (key, value) in BTreeMap::from([
            ("Thumb::URI", "file:///home/alice/photo.png"),
            ("Thumb::MTime", "42"),
            ("Thumb::Size", "12"),
            ("Thumb::Mimetype", "image/png"),
        ]) {
            encoder
                .add_text_chunk(key.to_owned(), value.to_owned())
                .unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![255; width as usize * height as usize * 4])
            .unwrap();
    }
    output
}
