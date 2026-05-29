// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, FailureNamespace, OriginalIdentity, PersonalCacheRoot, PersonalOriginalUri,
    ReadableOriginalIdentity, ThumbnailSize, UnixMTimeSeconds,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn generates_completion_without_scanning_cache() {
    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .args(["--generate-completion", "zsh"])
        .assert()
        .success()
        .stdout(predicates::str::contains("#compdef xdg-thumbnail-prune"));
}

#[test]
fn generates_manpage_before_delete_option_validation() {
    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .args(["--allow-delete-failures", "--generate-manpage"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Prune stale or invalid Freedesktop thumbnail cache entries",
        ));
}

#[test]
fn reports_missing_local_originals_without_deleting_by_default() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_for_missing_original();

    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .arg("--age-basis")
        .arg("modification-time")
        .assert()
        .success()
        .stdout(predicates::str::contains("would-delete"))
        .stdout(predicates::str::contains("original-missing"));

    assert!(thumbnail.exists());
}

#[test]
fn deletes_missing_local_originals_with_jsonl_report_when_requested() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_for_missing_original();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args([
            "--delete",
            "--format",
            "jsonl",
            "--age-basis",
            "modification-time",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines = String::from_utf8(output).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert!(!thumbnail.exists());
    assert_eq!(records[0]["event"], "entry");
    assert_eq!(records[0]["decision"], "delete");
    assert_eq!(records[0]["applied"], true);
    assert_eq!(records[0]["reason"], "original-missing");
    assert_eq!(records.last().unwrap()["event"], "summary");
    assert_eq!(records.last().unwrap()["deleted"], 1);
}

#[test]
fn failure_deletion_opt_in_requires_failure_scope() {
    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .arg("--allow-delete-failures")
        .assert()
        .code(2)
        .stderr(predicates::str::contains("--scope failures"));
}

#[test]
fn deletes_stale_local_failure_entries_when_both_stale_and_failure_deletion_are_enabled() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_stale_failure_entry();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args([
            "--scope",
            "failures",
            "--allow-delete-failures",
            "--delete-stale-local",
            "--delete",
            "--format",
            "jsonl",
            "--age-basis",
            "modification-time",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines = String::from_utf8(output).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert!(!thumbnail.exists());
    assert_eq!(records[0]["namespace"], "fail/app-1");
    assert_eq!(records[0]["decision"], "delete");
    assert_eq!(records[0]["applied"], true);
    assert_eq!(records[0]["reason"], "stale-local-metadata");
}

#[test]
fn reports_stale_local_thumbnails_with_stale_decision_until_delete_is_enabled() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_stale_local_thumbnail();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args([
            "--delete-stale-local",
            "--format",
            "jsonl",
            "--age-basis",
            "modification-time",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines = String::from_utf8(output).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert!(thumbnail.exists());
    assert_eq!(records[0]["decision"], "stale");
    assert_eq!(records[0]["applied"], false);
    assert_eq!(records[0]["reason"], "stale-local-metadata");

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args([
            "--delete-stale-local",
            "--delete",
            "--format",
            "jsonl",
            "--age-basis",
            "modification-time",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines = String::from_utf8(output).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert!(!thumbnail.exists());
    assert_eq!(records[0]["decision"], "delete");
    assert_eq!(records[0]["applied"], true);
    assert_eq!(records[0]["reason"], "stale-local-metadata");
}

#[test]
fn deletes_entries_whose_filename_does_not_match_stored_uri() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_uri_filename_mismatch();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args([
            "--delete",
            "--format",
            "jsonl",
            "--age-basis",
            "modification-time",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines = String::from_utf8(output).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert!(!thumbnail.exists());
    assert_eq!(records[0]["decision"], "delete");
    assert_eq!(records[0]["applied"], true);
    assert_eq!(records[0]["reason"], "uri-filename-mismatch");
}

#[test]
fn verbose_human_output_includes_kept_entries() {
    let fixture = Fixture::new();
    fixture.install_valid_local_thumbnail();

    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args(["--verbose", "--age-basis", "modification-time"])
        .assert()
        .success()
        .stdout(predicates::str::contains("keep"))
        .stdout(predicates::str::contains("local-stable-file"));
}

#[test]
fn nonconforming_entries_still_apply_missing_original_policy() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_nonconforming_for_missing_original();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args(["--format", "jsonl", "--age-basis", "modification-time"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines = String::from_utf8(output).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert!(thumbnail.exists());
    assert_eq!(records[0]["decision"], "delete");
    assert_eq!(records[0]["reason"], "original-missing");
}

#[cfg(unix)]
#[test]
fn human_output_reports_delete_failures_as_errors() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_for_missing_original();
    std::fs::set_permissions(
        thumbnail.parent().unwrap(),
        std::fs::Permissions::from_mode(0o500),
    )
    .unwrap();

    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args(["--delete", "--age-basis", "modification-time"])
        .assert()
        .code(1)
        .stdout(predicates::str::contains("delete-failed"))
        .stdout(predicates::str::contains("error=delete-failed"));

    std::fs::set_permissions(
        thumbnail.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
}

#[test]
fn nonfatal_inspection_errors_are_counted_in_summary_errors() {
    let fixture = Fixture::new();
    let dir = fixture.cache_home.path().join("thumbnails/normal");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir(dir.join("abcdefabcdefabcdefabcdefabcdefab.png")).unwrap();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args(["--format", "jsonl", "--age-basis", "modification-time"])
        .assert()
        .code(4)
        .get_output()
        .stdout
        .clone();
    let lines = String::from_utf8(output).unwrap();
    let records = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(records[0]["decision"], "skip");
    assert_eq!(records[0]["reason"], "unreadable-entry");
    assert_eq!(records.last().unwrap()["errors"], 1);
}

struct Fixture {
    cache_home: TempDir,
    home: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            cache_home: TempDir::new().unwrap(),
            home: TempDir::new().unwrap(),
        }
    }

    fn install_for_missing_original(&self) -> std::path::PathBuf {
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        let original = ReadableOriginalIdentity::from_confirmed_readable_identity(
            OriginalIdentity::with_mime_type(
                PersonalOriginalUri::from_absolute_path_bytes(b"/tmp/xdg-thumbnail-missing.png")
                    .unwrap(),
                UnixMTimeSeconds::new(42),
                Some(12),
                "image/png",
            )
            .unwrap(),
        );
        root.install_personal_thumbnail_png_bytes(
            &original,
            ThumbnailSize::Normal,
            &rendered_png(),
        )
        .unwrap();
        root.cache_entry_path(
            original.identity().uri(),
            &CacheNamespace::Size(ThumbnailSize::Normal),
        )
    }

    fn install_nonconforming_for_missing_original(&self) -> std::path::PathBuf {
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        let original = OriginalIdentity::with_mime_type(
            PersonalOriginalUri::from_absolute_path_bytes(b"/tmp/xdg-thumbnail-huge-missing.png")
                .unwrap(),
            UnixMTimeSeconds::new(42),
            Some(12),
            "image/png",
        )
        .unwrap();
        let path =
            root.cache_entry_path(original.uri(), &CacheNamespace::Size(ThumbnailSize::Normal));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, png_with_metadata(129, 1, &original)).unwrap();
        path
    }

    fn install_stale_failure_entry(&self) -> std::path::PathBuf {
        let original_path = self.home.path().join("original.png");
        std::fs::write(&original_path, b"new original").unwrap();
        let uri = PersonalOriginalUri::from_absolute_path_bytes(
            original_path.as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        let original = ReadableOriginalIdentity::from_confirmed_readable_identity(
            OriginalIdentity::with_mime_type(uri, UnixMTimeSeconds::new(1), Some(1), "image/png")
                .unwrap(),
        );
        let namespace = FailureNamespace::new("app-1").unwrap();
        root.write_failure_entry_png_bytes(&namespace, &original)
            .unwrap();
        root.cache_entry_path(
            original.identity().uri(),
            &CacheNamespace::Failure(namespace),
        )
    }

    fn install_uri_filename_mismatch(&self) -> std::path::PathBuf {
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        let original = ReadableOriginalIdentity::from_confirmed_readable_identity(
            OriginalIdentity::with_mime_type(
                PersonalOriginalUri::from_absolute_path_bytes(b"/tmp/xdg-thumbnail-photo.png")
                    .unwrap(),
                UnixMTimeSeconds::new(42),
                Some(12),
                "image/png",
            )
            .unwrap(),
        );
        let installed = root
            .install_personal_thumbnail_png_bytes(&original, ThumbnailSize::Normal, &rendered_png())
            .unwrap();
        let wrong_uri =
            PersonalOriginalUri::from_absolute_path_bytes(b"/tmp/xdg-thumbnail-other.png").unwrap();
        let mismatched =
            root.cache_entry_path(&wrong_uri, &CacheNamespace::Size(ThumbnailSize::Normal));
        std::fs::rename(installed.path(), &mismatched).unwrap();
        mismatched
    }

    fn install_stale_local_thumbnail(&self) -> std::path::PathBuf {
        let original_path = self.home.path().join("stale-original.png");
        std::fs::write(&original_path, b"current original").unwrap();
        let uri = PersonalOriginalUri::from_absolute_path_bytes(
            original_path.as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        let original = ReadableOriginalIdentity::from_confirmed_readable_identity(
            OriginalIdentity::with_mime_type(uri, UnixMTimeSeconds::new(1), Some(1), "image/png")
                .unwrap(),
        );
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        root.install_personal_thumbnail_png_bytes(
            &original,
            ThumbnailSize::Normal,
            &rendered_png(),
        )
        .unwrap();
        root.cache_entry_path(
            original.identity().uri(),
            &CacheNamespace::Size(ThumbnailSize::Normal),
        )
    }

    fn install_valid_local_thumbnail(&self) -> std::path::PathBuf {
        let original_path = self.home.path().join("original.png");
        std::fs::write(&original_path, b"original").unwrap();
        let metadata = std::fs::metadata(&original_path).unwrap();
        let uri = PersonalOriginalUri::from_absolute_path_bytes(
            original_path.as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        let mtime = UnixMTimeSeconds::from_system_time(metadata.modified().unwrap()).unwrap();
        let original = ReadableOriginalIdentity::from_confirmed_readable_identity(
            OriginalIdentity::with_mime_type(uri, mtime, Some(metadata.len()), "image/png")
                .unwrap(),
        );
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        root.install_personal_thumbnail_png_bytes(
            &original,
            ThumbnailSize::Normal,
            &rendered_png(),
        )
        .unwrap();
        root.cache_entry_path(
            original.identity().uri(),
            &CacheNamespace::Size(ThumbnailSize::Normal),
        )
    }
}

fn rendered_png() -> Vec<u8> {
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

fn png_with_metadata(width: u32, height: u32, original: &OriginalIdentity) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_text_chunk("Thumb::URI".to_owned(), original.uri().as_str().to_owned())
            .unwrap();
        encoder
            .add_text_chunk("Thumb::MTime".to_owned(), original.mtime().to_string())
            .unwrap();
        if let Some(size) = original.original_byte_size() {
            encoder
                .add_text_chunk("Thumb::Size".to_owned(), size.to_string())
                .unwrap();
        }
        if let Some(mime_type) = original.mime_type() {
            encoder
                .add_text_chunk("Thumb::Mimetype".to_owned(), mime_type.to_owned())
                .unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![255; width as usize * height as usize * 4])
            .unwrap();
    }
    output
}
