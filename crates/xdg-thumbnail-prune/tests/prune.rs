// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, CacheRoot, OriginalIdentity, PersonalThumbnailUri, ReadableOriginalIdentity,
    ThumbnailSize, UnixMTimeSeconds,
};

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
        let root = CacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        let original = ReadableOriginalIdentity::new(
            OriginalIdentity::new(
                PersonalThumbnailUri::from_absolute_path_bytes(b"/tmp/xdg-thumbnail-missing.png")
                    .unwrap(),
                UnixMTimeSeconds::new(42),
                Some(12),
                Some("image/png"),
            )
            .unwrap(),
        );
        root.install_personal_thumbnail(&original, ThumbnailSize::Normal, &rendered_png())
            .unwrap();
        root.personal_path(
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
