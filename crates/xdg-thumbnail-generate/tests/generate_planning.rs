// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;
use xdg_thumbnail::{
    CacheRoot, OriginalIdentity, PersonalThumbnailUri, ReadableOriginalIdentity, ThumbnailSize,
    UnixMTimeSeconds,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn dry_run_reports_selected_thumbnailer_and_target_cache_path() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    fixture.write_thumbnailer("test.thumbnailer", "/bin/true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    assert_eq!(records[0]["event"], "entry");
    assert_eq!(records[0]["decision"], "generate");
    assert_eq!(records[0]["applied"], false);
    assert_eq!(records[0]["reason"], "dry-run");
    assert_eq!(records[0]["mime_type"], "image/png");
    assert_eq!(records[0]["thumbnailer"], "test.thumbnailer");
    assert_eq!(records[0]["sandbox_mode"], "off");
    assert!(
        records[0]["cache_path_display"]
            .as_str()
            .unwrap()
            .contains("/thumbnails/normal/")
    );
}

#[test]
fn dry_run_keeps_existing_valid_thumbnail_unless_forced() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("existing.png");
    fixture.install_existing_thumbnail(&input);
    fixture.write_thumbnailer("test.thumbnailer", "/bin/true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    assert_eq!(records[0]["decision"], "keep");
    assert_eq!(records[0]["reason"], "already-valid");
}

#[test]
fn rejects_inputs_inside_personal_cache() {
    let fixture = Fixture::new();
    let cache_input = fixture
        .cache_home
        .path()
        .join("thumbnails/normal/input.png");
    std::fs::create_dir_all(cache_input.parent().unwrap()).unwrap();
    std::fs::write(&cache_input, rendered_png()).unwrap();

    let assert = fixture.command([
        "--dry-run",
        "--sandbox",
        "off",
        cache_input.to_str().unwrap(),
    ]);
    assert
        .code(4)
        .stdout(predicates::str::contains("unsupported-input"));
}

#[cfg(unix)]
#[test]
fn try_exec_without_execute_permission_is_ignored() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    let helper = fixture.root.path().join("not-executable-thumbnailer");
    std::fs::write(&helper, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o644)).unwrap();
    fixture.write_thumbnailer_with_try_exec(
        "blocked.thumbnailer",
        &format!("{} %i %o %s", helper.display()),
        "image/png;",
        helper.to_str().unwrap(),
    );

    let assert = fixture.command([
        "--dry-run",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    assert
        .code(4)
        .stdout(predicates::str::contains("no-matching-thumbnailer"));
}

struct Fixture {
    root: TempDir,
    cache_home: TempDir,
    home: TempDir,
    data_home: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: TempDir::new().unwrap(),
            cache_home: TempDir::new().unwrap(),
            home: TempDir::new().unwrap(),
            data_home: TempDir::new().unwrap(),
        }
    }

    fn command<const N: usize>(&self, args: [&str; N]) -> assert_cmd::assert::Assert {
        let mut command = Command::cargo_bin("xdg-thumbnail-generate").unwrap();
        command
            .env("XDG_CACHE_HOME", self.cache_home.path())
            .env("HOME", self.home.path())
            .env("XDG_DATA_HOME", self.data_home.path())
            .env("XDG_DATA_DIRS", "")
            .args(args);
        command.assert()
    }

    fn run_jsonl<const N: usize>(&self, args: [&str; N]) -> Vec<Value> {
        let output = self.command(args).success().get_output().stdout.clone();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect()
    }

    fn write_png_input(&self, name: &str) -> std::path::PathBuf {
        let path = self.root.path().join(name);
        std::fs::write(&path, rendered_png()).unwrap();
        path
    }

    fn write_thumbnailer(&self, name: &str, exec: &str, mime_type: &str) {
        let dir = self.data_home.path().join("thumbnailers");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(name),
            format!("[Thumbnailer Entry]\nExec={exec}\nMimeType={mime_type}\n"),
        )
        .unwrap();
    }

    fn write_thumbnailer_with_try_exec(
        &self,
        name: &str,
        exec: &str,
        mime_type: &str,
        try_exec: &str,
    ) {
        let dir = self.data_home.path().join("thumbnailers");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(name),
            format!("[Thumbnailer Entry]\nExec={exec}\nMimeType={mime_type}\nTryExec={try_exec}\n"),
        )
        .unwrap();
    }

    fn install_existing_thumbnail(&self, input: &std::path::Path) {
        let metadata = std::fs::metadata(input).unwrap();
        let mtime = UnixMTimeSeconds::from_system_time(metadata.modified().unwrap()).unwrap();
        let uri =
            PersonalThumbnailUri::from_absolute_path_bytes(input.as_os_str().as_encoded_bytes())
                .unwrap();
        let original = ReadableOriginalIdentity::new(
            OriginalIdentity::new(uri, mtime, Some(metadata.len()), Some("image/png")).unwrap(),
        );
        CacheRoot::new(self.cache_home.path().join("thumbnails"))
            .unwrap()
            .install_personal_thumbnail(&original, ThumbnailSize::Normal, &rendered_png())
            .unwrap();
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
