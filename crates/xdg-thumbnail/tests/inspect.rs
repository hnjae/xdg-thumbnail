// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::collections::BTreeMap;
use std::os::unix::fs::symlink;

use tempfile::TempDir;
use xdg_thumbnail::{
    AccessTimePreservation, CacheEntryInspectionOutcome, CacheEntryProblem, CacheNamespace,
    FailureNamespace, NonstandardEntryPolicy, OriginalIdentity, OriginalUriIdentity,
    PersonalCacheRoot, PersonalOriginalUri, ReadableOriginalIdentity, ThumbnailSize,
};

#[test]
fn inspection_iterates_standard_entries_and_reports_facts() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let installed = root
        .install_personal_thumbnail_bytes(
            &original,
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1),
        )
        .unwrap();
    let nonstandard_dir = root.as_path().join("normal");
    std::fs::write(nonstandard_dir.join("note.txt"), b"not a thumbnail").unwrap();

    let default_entries = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], NonstandardEntryPolicy::Exclude)
        .unwrap();
    assert_eq!(default_entries.len(), 1);
    assert_eq!(default_entries[0].path(), installed.path());
    assert_eq!(
        default_entries[0].namespace(),
        &CacheNamespace::Size(ThumbnailSize::Normal)
    );
    assert_eq!(
        default_entries[0].outcome(),
        &CacheEntryInspectionOutcome::Unchecked
    );
    assert!(default_entries[0].timestamps().modified_at().is_some());
    assert_eq!(
        default_entries[0]
            .timestamps()
            .access_time_preserved_during_inspection(),
        AccessTimePreservation::Preserved
    );
    assert!(matches!(
        default_entries[0].original_uri(),
        Some(OriginalUriIdentity::Personal(uri)) if uri.as_str() == "file:///home/alice/photo.png"
    ));

    let visible_entries = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], NonstandardEntryPolicy::Include)
        .unwrap();
    assert_eq!(visible_entries.len(), 2);
    assert!(visible_entries.iter().any(|entry| {
        matches!(entry.outcome(), CacheEntryInspectionOutcome::Invalid(problems) if problems.contains(&CacheEntryProblem::NonstandardFilename))
    }));
}

#[test]
fn inspection_reports_invalid_uri_metadata_and_filename_uri_mismatch() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let dir = root.as_path().join("normal");
    std::fs::create_dir_all(&dir).unwrap();

    let invalid_uri_path = dir.join("abcdefabcdefabcdefabcdefabcdefab.png");
    std::fs::write(
        &invalid_uri_path,
        png_with_metadata(
            2,
            1,
            BTreeMap::from([
                ("Thumb::URI", "file:///home/alice/My Photo.png"),
                ("Thumb::MTime", "42"),
            ]),
        ),
    )
    .unwrap();

    let expected_uri =
        PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap();
    let wrong_uri = PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/other.png")
        .unwrap()
        .thumbnail_filename();
    let mismatched_path = dir.join(wrong_uri);
    std::fs::write(
        &mismatched_path,
        png_with_metadata(
            2,
            1,
            BTreeMap::from([
                ("Thumb::URI", expected_uri.as_str()),
                ("Thumb::MTime", "42"),
            ]),
        ),
    )
    .unwrap();

    let entries = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], NonstandardEntryPolicy::Exclude)
        .unwrap();

    assert!(entries.iter().any(|entry| {
        entry.path() == invalid_uri_path.as_path()
            && matches!(entry.outcome(), CacheEntryInspectionOutcome::Invalid(problems) if problems.contains(&CacheEntryProblem::InvalidMetadataSyntax))
    }));
    assert!(entries.iter().any(|entry| {
        entry.path() == mismatched_path.as_path()
            && matches!(entry.outcome(), CacheEntryInspectionOutcome::Invalid(problems) if problems.contains(&CacheEntryProblem::UriFilenameMismatch))
    }));
}

#[test]
fn failure_iteration_is_limited_to_one_real_namespace_level() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let namespace = FailureNamespace::new("app-1").unwrap();
    root.write_failure_entry_bytes(&namespace, &original)
        .unwrap();

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

    let entries = root
        .inspect_failure_entries(NonstandardEntryPolicy::Exclude)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].namespace(),
        &CacheNamespace::Failure(FailureNamespace::new("app-1").unwrap())
    );
}

#[test]
fn inspection_does_not_follow_symlinked_size_namespace_directories() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
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
        .inspect_thumbnails(&[ThumbnailSize::Normal], NonstandardEntryPolicy::Exclude)
        .unwrap();

    assert!(entries.is_empty());
}

#[test]
fn cache_entry_handles_remove_files_without_following_symlinks() {
    let temp = TempDir::new().unwrap();
    let root = PersonalCacheRoot::new(temp.path().join("thumbnails")).unwrap();
    let original = readable_original();
    let installed = root
        .install_personal_thumbnail_bytes(
            &original,
            ThumbnailSize::Normal,
            &png_without_metadata(2, 1),
        )
        .unwrap();
    let handle = root
        .inspect_thumbnails(&[ThumbnailSize::Normal], NonstandardEntryPolicy::Exclude)
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
        .inspect_thumbnails(&[ThumbnailSize::Normal], NonstandardEntryPolicy::Exclude)
        .unwrap()[0]
        .handle()
        .clone();
    assert!(symlink_handle.remove().is_err());
    assert!(outside.exists());
}

fn readable_original() -> ReadableOriginalIdentity {
    ReadableOriginalIdentity::from_confirmed_readable_identity(
        OriginalIdentity::with_mime_type(
            PersonalOriginalUri::from_absolute_path_bytes(b"/home/alice/photo.png").unwrap(),
            xdg_thumbnail::UnixMTimeSeconds::new(42),
            Some(12),
            "image/png",
        )
        .unwrap(),
    )
}

fn png_without_metadata(width: u32, height: u32) -> Vec<u8> {
    png_with_metadata(
        width,
        height,
        BTreeMap::from([
            ("Thumb::URI", "file:///home/alice/photo.png"),
            ("Thumb::MTime", "42"),
            ("Thumb::Size", "12"),
            ("Thumb::Mimetype", "image/png"),
        ]),
    )
}

fn png_with_metadata(width: u32, height: u32, metadata: BTreeMap<&str, &str>) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        for (key, value) in metadata {
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
