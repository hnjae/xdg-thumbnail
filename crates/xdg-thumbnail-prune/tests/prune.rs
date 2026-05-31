// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, FailureNamespace, PersonalCacheRoot, PersonalOriginalIdentity,
    PersonalOriginalUri, ReadablePersonalOriginalIdentity, ThumbnailSize, UnixMtimeSeconds,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn help_and_version_are_successful_metadata_modes() {
    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: xdg-thumbnail-prune [OPTIONS]",
        ))
        .stdout(predicate::str::contains("Apply deletion decisions"))
        .stdout(predicate::str::contains(
            "Actual deletion still requires --delete",
        ))
        .stdout(predicate::str::contains("--allow-stale-local-deletion"))
        .stdout(predicate::str::contains("--allow-failure-deletion"))
        .stdout(predicate::str::contains(
            "Requires --scope failures or --scope all",
        ))
        .stdout(predicate::str::contains("--ignore-media-prefix"))
        .stdout(predicate::str::contains("--delete-stale-local").not())
        .stdout(predicate::str::contains("--allow-delete-failures").not())
        .stdout(predicate::str::contains("--ignore-fhs-media").not());

    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("xdg-thumbnail-prune"));
}

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
        .args(["--allow-failure-deletion", "--generate-manpage"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Prune stale or invalid Freedesktop thumbnail cache entries",
        ));
}

#[test]
fn default_age_basis_is_reported_as_atime() {
    let fixture = Fixture::new();

    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("basis=atime"));
}

#[test]
fn duplicate_explicit_sizes_are_scanned_once() {
    let fixture = Fixture::new();
    fixture.install_valid_local_thumbnail();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args([
            "--size",
            "normal",
            "--size",
            "normal",
            "--format",
            "jsonl",
            "--age-basis",
            "mtime",
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

    let entries = records
        .iter()
        .filter(|record| record["event"] == "entry")
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["namespace"], "normal");
    assert_eq!(records.last().unwrap()["scanned"], 1);
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
        .arg("mtime")
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
        .args(["--delete", "--format", "jsonl", "--age-basis", "mtime"])
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
    assert_eq!(records[0]["age_basis"], "mtime");
    assert_eq!(records.last().unwrap()["event"], "summary");
    assert_eq!(records.last().unwrap()["deleted"], 1);
    assert_eq!(records.last().unwrap()["age_basis"], "mtime");
}

#[test]
fn failure_deletion_opt_in_requires_failure_scope() {
    Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .arg("--allow-failure-deletion")
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "--allow-failure-deletion requires --scope failures or --scope all",
        ));
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
            "--allow-failure-deletion",
            "--allow-stale-local-deletion",
            "--delete",
            "--format",
            "jsonl",
            "--age-basis",
            "mtime",
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
fn allow_stale_local_deletion_previews_stale_local_thumbnails_until_delete_is_enabled() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_stale_local_thumbnail();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args([
            "--allow-stale-local-deletion",
            "--format",
            "jsonl",
            "--age-basis",
            "mtime",
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
    assert_eq!(records[0]["decision"], "delete");
    assert_eq!(records[0]["applied"], false);
    assert_eq!(records[0]["reason"], "stale-local-metadata");
    assert_eq!(records.last().unwrap()["would_delete"], 1);

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args([
            "--allow-stale-local-deletion",
            "--delete",
            "--format",
            "jsonl",
            "--age-basis",
            "mtime",
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
        .args(["--delete", "--format", "jsonl", "--age-basis", "mtime"])
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
        .args(["--verbose", "--age-basis", "mtime"])
        .assert()
        .success()
        .stdout(predicates::str::contains("keep"))
        .stdout(predicates::str::contains("local-stable-file"));
}

#[test]
fn keeps_local_thumbnail_without_optional_metadata() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_valid_local_thumbnail_without_optional_metadata();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args(["--format", "jsonl", "--age-basis", "mtime"])
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
    assert_eq!(records[0]["decision"], "keep");
    assert_eq!(records[0]["reason"], Value::Null);
}

#[test]
fn nonconforming_entries_still_apply_missing_original_policy() {
    let fixture = Fixture::new();
    let thumbnail = fixture.install_nonconforming_for_missing_original();

    let output = Command::cargo_bin("xdg-thumbnail-prune")
        .unwrap()
        .env("XDG_CACHE_HOME", fixture.cache_home.path())
        .env("HOME", fixture.home.path())
        .args(["--format", "jsonl", "--age-basis", "mtime"])
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
        .args(["--delete", "--age-basis", "mtime"])
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
        .args(["--format", "jsonl", "--age-basis", "mtime"])
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
        let original = ReadablePersonalOriginalIdentity::assume_readable(
            PersonalOriginalIdentity::new(
                PersonalOriginalUri::from_absolute_path_bytes(b"/tmp/xdg-thumbnail-missing.png")
                    .unwrap(),
                UnixMtimeSeconds::new(42),
            )
            .with_original_byte_size(12)
            .with_mime_type("image/png")
            .unwrap(),
        );
        root.install_thumbnail_returning_png_bytes(
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
        let original = PersonalOriginalIdentity::new(
            PersonalOriginalUri::from_absolute_path_bytes(b"/tmp/xdg-thumbnail-huge-missing.png")
                .unwrap(),
            UnixMtimeSeconds::new(42),
        )
        .with_original_byte_size(12)
        .with_mime_type("image/png")
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
        let original = ReadablePersonalOriginalIdentity::assume_readable(
            PersonalOriginalIdentity::new(uri, UnixMtimeSeconds::new(1))
                .with_original_byte_size(1)
                .with_mime_type("image/png")
                .unwrap(),
        );
        let namespace = FailureNamespace::new("app-1").unwrap();
        root.write_failure_entry_returning_png_bytes(&original, &namespace)
            .unwrap();
        root.cache_entry_path(
            original.identity().uri(),
            &CacheNamespace::Failure(namespace),
        )
    }

    fn install_uri_filename_mismatch(&self) -> std::path::PathBuf {
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        let original = ReadablePersonalOriginalIdentity::assume_readable(
            PersonalOriginalIdentity::new(
                PersonalOriginalUri::from_absolute_path_bytes(b"/tmp/xdg-thumbnail-photo.png")
                    .unwrap(),
                UnixMtimeSeconds::new(42),
            )
            .with_original_byte_size(12)
            .with_mime_type("image/png")
            .unwrap(),
        );
        let installed = root
            .install_thumbnail_returning_png_bytes(
                &original,
                ThumbnailSize::Normal,
                &rendered_png(),
            )
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
        let original = ReadablePersonalOriginalIdentity::assume_readable(
            PersonalOriginalIdentity::new(uri, UnixMtimeSeconds::new(1))
                .with_original_byte_size(1)
                .with_mime_type("image/png")
                .unwrap(),
        );
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        root.install_thumbnail_returning_png_bytes(
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
        let mtime = UnixMtimeSeconds::from_system_time(metadata.modified().unwrap()).unwrap();
        let original = ReadablePersonalOriginalIdentity::assume_readable(
            PersonalOriginalIdentity::new(uri, mtime)
                .with_original_byte_size(metadata.len())
                .with_mime_type("image/png")
                .unwrap(),
        );
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        root.install_thumbnail_returning_png_bytes(
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

    fn install_valid_local_thumbnail_without_optional_metadata(&self) -> std::path::PathBuf {
        let original_path = self.home.path().join("original-without-optional.png");
        std::fs::write(&original_path, b"original").unwrap();
        let metadata = std::fs::metadata(&original_path).unwrap();
        let uri = PersonalOriginalUri::from_absolute_path_bytes(
            original_path.as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        let mtime = UnixMtimeSeconds::from_system_time(metadata.modified().unwrap()).unwrap();
        let original = PersonalOriginalIdentity::new(uri, mtime);
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        let path =
            root.cache_entry_path(original.uri(), &CacheNamespace::Size(ThumbnailSize::Normal));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, png_with_metadata(2, 1, &original)).unwrap();
        path
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

fn png_with_metadata(width: u32, height: u32, original: &PersonalOriginalIdentity) -> Vec<u8> {
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
