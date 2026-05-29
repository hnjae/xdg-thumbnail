// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;
use xdg_thumbnail::ParsedThumbnailPng;

#[test]
fn sandbox_off_executes_thumbnailer_and_installs_output() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    let script = fixture.write_script("copy-thumbnailer", "#!/bin/sh\ncp \"$1\" \"$2\"\n");
    fixture.write_thumbnailer(
        "copy.thumbnailer",
        &format!("{} %i %o %s", script.display()),
        "image/png;",
    );

    let records = fixture.run_jsonl([
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    assert_eq!(records[0]["decision"], "generated");
    assert_eq!(records[0]["applied"], true);
    assert_eq!(records[0]["reason"], "created");
    let cache_path = records[0]["cache_path_display"].as_str().unwrap();
    let bytes = std::fs::read(cache_path).unwrap();
    let parsed = ParsedThumbnailPng::parse(&bytes).unwrap();
    assert_eq!(parsed.metadata().thumb_uri(), records[0]["uri"].as_str());
    assert_eq!(parsed.metadata().thumb_mimetype(), Some("image/png"));
}

#[test]
fn thumbnailer_timeout_is_reported_without_installing_output() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("slow.png");
    let script = fixture.write_script("slow-thumbnailer", "#!/bin/sh\nsleep 2\ncp \"$1\" \"$2\"\n");
    fixture.write_thumbnailer(
        "slow.thumbnailer",
        &format!("{} %i %o %s", script.display()),
        "image/png;",
    );

    let output = fixture
        .command([
            "--sandbox",
            "off",
            "--timeout",
            "1s",
            "--format",
            "jsonl",
            input.to_str().unwrap(),
        ])
        .code(1)
        .get_output()
        .stdout
        .clone();
    let records = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(records[0]["decision"], "failed");
    assert_eq!(records[0]["reason"], "thumbnailer-timeout");
    assert_eq!(records[0]["applied"], false);
    assert_eq!(records[0]["sandbox_applied"], false);
}

#[test]
fn required_sandbox_execution_failures_report_sandbox_applied() {
    let fixture = Fixture::new();
    fixture.write_script("bwrap", "#!/bin/sh\nexit 0\n");
    let input = fixture.write_png_input("sandboxed.png");
    fixture.write_thumbnailer("missing.thumbnailer", "true %i %o %s", "image/png;");

    let output = fixture
        .command(["--format", "jsonl", input.to_str().unwrap()])
        .code(1)
        .get_output()
        .stdout
        .clone();
    let records = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(records[0]["decision"], "failed");
    assert_eq!(records[0]["reason"], "thumbnailer-output-missing");
    assert_eq!(records[0]["sandbox_applied"], true);
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
        let mut path = OsString::from(self.root.path());
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        command
            .env("XDG_CACHE_HOME", self.cache_home.path())
            .env("HOME", self.home.path())
            .env("XDG_DATA_HOME", self.data_home.path())
            .env("XDG_DATA_DIRS", "")
            .env("PATH", path)
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

    fn write_script(&self, name: &str, content: &str) -> std::path::PathBuf {
        let path = self.root.path().join(name);
        std::fs::write(&path, content).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
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
