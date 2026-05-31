// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use assert_cmd::Command;
use base64::Engine;
use predicates::prelude::*;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use tempfile::TempDir;
use xdg_thumbnail::{
    CacheNamespace, PersonalCacheRoot, PersonalOriginalIdentity, PersonalOriginalUri,
    ReadablePersonalOriginalIdentity, ThumbnailSize, UnixMtimeSeconds,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn help_and_version_are_successful_metadata_modes() {
    Command::cargo_bin("xdg-thumbnail-generate")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: xdg-thumbnail-generate [OPTIONS] <PATH>...",
        ))
        .stdout(predicate::str::contains(
            "Report planned thumbnailer selection",
        ))
        .stdout(predicate::str::contains("Required for generation"))
        .stdout(predicate::str::contains("--generate-completion"))
        .stdout(predicate::str::contains("--generate-manpage"))
        .stdout(predicate::str::contains("without PATH operands"))
        .stdout(predicate::str::contains("--verbose").not());

    Command::cargo_bin("xdg-thumbnail-generate")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("xdg-thumbnail-generate"));
}

#[test]
fn generates_completion_without_inputs() {
    Command::cargo_bin("xdg-thumbnail-generate")
        .unwrap()
        .args(["--generate-completion", "bash"])
        .assert()
        .success()
        .stdout(predicates::str::contains("_xdg-thumbnail-generate"));
}

#[test]
fn generates_manpage_without_inputs() {
    Command::cargo_bin("xdg-thumbnail-generate")
        .unwrap()
        .arg("--generate-manpage")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Generate Freedesktop thumbnail cache entries",
        ))
        .stdout(predicate::str::contains("Required for generation"));
}

#[test]
fn generation_requires_input_path() {
    Command::cargo_bin("xdg-thumbnail-generate")
        .unwrap()
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "required arguments were not provided",
        ));
}

#[test]
fn verbose_is_not_a_generate_option() {
    Command::cargo_bin("xdg-thumbnail-generate")
        .unwrap()
        .args(["--verbose", "input.png"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--verbose"));
}

#[test]
fn dry_run_reports_selected_thumbnailer_and_target_cache_path() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    fixture.write_thumbnailer("test.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
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
    assert_eq!(records.last().unwrap()["event"], "summary");
    assert_eq!(records.last().unwrap()["planned"], 1);
    assert_eq!(records.last().unwrap()["generated"], 0);
}

#[test]
fn dry_run_defaults_to_all_supported_sizes() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    fixture.write_thumbnailer("test.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    let entries = records
        .iter()
        .filter(|record| record["event"] == "entry")
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0]["namespace"], "normal");
    assert_eq!(entries[1]["namespace"], "large");
    assert_eq!(entries[2]["namespace"], "x-large");
    assert_eq!(entries[3]["namespace"], "xx-large");
    assert!(entries.iter().all(|entry| entry["decision"] == "generate"));
    assert_eq!(records.last().unwrap()["requested"], 4);
    assert_eq!(records.last().unwrap()["planned"], 4);
}

#[test]
fn duplicate_explicit_sizes_are_planned_once() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    fixture.write_thumbnailer("test.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
        "--size",
        "normal",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    let entries = records
        .iter()
        .filter(|record| record["event"] == "entry")
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["namespace"], "normal");
    assert_eq!(records.last().unwrap()["requested"], 1);
    assert_eq!(records.last().unwrap()["planned"], 1);
}

#[cfg(unix)]
#[test]
fn cache_lookup_failure_uses_stable_reason() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    let normal_dir = fixture.cache_home.path().join("thumbnails/normal");
    std::fs::create_dir_all(&normal_dir).unwrap();
    std::fs::set_permissions(&normal_dir, std::fs::Permissions::from_mode(0o000)).unwrap();

    let output = fixture
        .command([
            "--dry-run",
            "--size",
            "normal",
            "--sandbox",
            "off",
            "--format",
            "jsonl",
            input.to_str().unwrap(),
        ])
        .code(4)
        .get_output()
        .stdout
        .clone();
    std::fs::set_permissions(&normal_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let records = parse_jsonl(output);

    assert_eq!(records[0]["decision"], "skip");
    assert_eq!(records[0]["reason"], "cache-lookup-failed");
    assert_eq!(records[0]["error"]["kind"], "cache-lookup-failed");
    assert_eq!(records.last().unwrap()["skipped"], 1);
}

#[test]
fn dry_run_keeps_existing_valid_thumbnail_unless_forced() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("existing.png");
    fixture.install_existing_thumbnail(&input);
    fixture.write_thumbnailer("test.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
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
fn dry_run_keeps_existing_valid_thumbnail_without_optional_metadata() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("existing.png");
    fixture.install_existing_thumbnail_without_optional_metadata(&input);
    fixture.write_thumbnailer("test.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
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

#[test]
fn dry_run_normalizes_relative_input_dot_segments_for_identity_and_reports() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    std::fs::create_dir(fixture.root.path().join("subdir")).unwrap();
    fixture.write_thumbnailer("test.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        "subdir/.././photo.png",
    ]);

    assert_eq!(records[0]["decision"], "generate");
    assert_entry_uses_input_identity(&fixture, &records[0], &input);
}

#[test]
fn dry_run_normalizes_absolute_input_dot_segments_for_identity_and_reports() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    std::fs::create_dir(fixture.root.path().join("dir")).unwrap();
    fixture.write_thumbnailer("test.thumbnailer", "true %i %o %s", "image/png;");
    let dot_segment_input = fixture.root.path().join("dir/.././photo.png");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        dot_segment_input.to_str().unwrap(),
    ]);

    assert_eq!(records[0]["decision"], "generate");
    assert_entry_uses_input_identity(&fixture, &records[0], &input);
}

#[cfg(unix)]
#[test]
fn dry_run_preserves_symlink_path_as_input_identity() {
    let fixture = Fixture::new();
    let real_dir = fixture.root.path().join("real");
    let link_dir = fixture.root.path().join("link");
    std::fs::create_dir(&real_dir).unwrap();
    std::fs::write(real_dir.join("photo.png"), rendered_png()).unwrap();
    std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap();
    fixture.write_thumbnailer("test.thumbnailer", "true %i %o %s", "image/png;");
    let symlink_input = link_dir.join("photo.png");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        symlink_input.to_str().unwrap(),
    ]);

    assert_eq!(records[0]["decision"], "generate");
    assert_entry_uses_input_identity(&fixture, &records[0], &symlink_input);
    assert!(
        !records[0]["uri"]
            .as_str()
            .unwrap()
            .contains("/real/photo.png")
    );
}

#[test]
fn rejects_dot_segment_inputs_inside_personal_cache() {
    let fixture = Fixture::new();
    let cache_input = fixture
        .cache_home
        .path()
        .join("thumbnails/normal/input.png");
    std::fs::create_dir_all(cache_input.parent().unwrap()).unwrap();
    std::fs::write(&cache_input, rendered_png()).unwrap();
    let cache_home_name = fixture.cache_home.path().file_name().unwrap();
    let dot_segment_input = fixture
        .cache_home
        .path()
        .join("..")
        .join(cache_home_name)
        .join("thumbnails/normal/input.png");

    let records = fixture.run_jsonl_code(
        [
            "--dry-run",
            "--size",
            "normal",
            "--sandbox",
            "off",
            "--format",
            "jsonl",
            dot_segment_input.to_str().unwrap(),
        ],
        4,
    );

    assert_eq!(records[0]["decision"], "skip");
    assert_eq!(records[0]["reason"], "unsupported-input");
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
        "--size",
        "normal",
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

#[test]
fn matching_invalid_thumbnailer_without_valid_match_reports_entry_error() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    fixture.write_invalid_thumbnailer("broken.thumbnailer", "image/png;");

    let records = fixture.run_jsonl_code(
        [
            "--dry-run",
            "--size",
            "normal",
            "--sandbox",
            "off",
            "--format",
            "jsonl",
            input.to_str().unwrap(),
        ],
        4,
    );

    assert_eq!(records[0]["event"], "entry");
    assert_eq!(records[0]["decision"], "skip");
    assert_eq!(records[0]["reason"], "thumbnailer-entry-invalid");
    assert_eq!(records[0]["thumbnailer"], "broken.thumbnailer");
}

#[test]
fn matching_invalid_thumbnailer_with_valid_match_emits_warning() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    fixture.write_invalid_thumbnailer("broken.thumbnailer", "image/png;");
    fixture.write_thumbnailer("valid.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    assert_eq!(records[0]["event"], "entry");
    assert_eq!(records[0]["decision"], "generate");
    assert_eq!(records[0]["thumbnailer"], "valid.thumbnailer");
    assert_eq!(records[1]["event"], "warning");
    assert_eq!(records[1]["thumbnailer"], "broken.thumbnailer");
    assert_eq!(records[1]["reason"], "thumbnailer-entry-invalid");
    assert_eq!(records.last().unwrap()["warnings"], 1);
}

#[test]
fn unrelated_invalid_thumbnailer_discovery_emits_warning() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    fixture.write_raw_thumbnailer("unrelated.thumbnailer", "[Desktop Entry]\nName=Broken\n");
    fixture.write_thumbnailer("valid.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    assert_eq!(records[0]["event"], "entry");
    assert_eq!(records[0]["decision"], "generate");
    assert_eq!(records[1]["event"], "warning");
    assert_eq!(records[1]["input_path_display"], Value::Null);
    assert_eq!(records[1]["thumbnailer"], "unrelated.thumbnailer");
    assert_eq!(records[1]["reason"], "thumbnailer-entry-invalid");
    assert_eq!(records.last().unwrap()["warnings"], 1);
}

#[test]
fn thumbnailer_matching_accepts_mime_supertypes() {
    let fixture = Fixture::new();
    let input = fixture.write_png_input("photo.png");
    fixture.write_thumbnailer("image.thumbnailer", "true %i %o %s", "image/*;");

    let records = fixture.run_jsonl([
        "--dry-run",
        "--size",
        "normal",
        "--sandbox",
        "off",
        "--format",
        "jsonl",
        input.to_str().unwrap(),
    ]);

    assert_eq!(records[0]["decision"], "generate");
    assert_eq!(records[0]["thumbnailer"], "image.thumbnailer");
}

#[cfg(unix)]
#[test]
fn required_sandbox_reports_backend_probe_failure_before_execution() {
    let fixture = Fixture::new();
    fixture.write_executable("bwrap", "#!/bin/sh\nexit 99\n");
    let input = fixture.write_png_input("photo.png");
    fixture.write_thumbnailer("valid.thumbnailer", "true %i %o %s", "image/png;");

    let records = fixture.run_jsonl_code(
        [
            "--dry-run",
            "--size",
            "normal",
            "--format",
            "jsonl",
            input.to_str().unwrap(),
        ],
        1,
    );

    assert_eq!(records[0]["decision"], "failed");
    assert_eq!(records[0]["reason"], "sandbox-unavailable");
    assert!(
        records[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("requires Linux bubblewrap support")
    );
}

#[cfg(unix)]
#[test]
fn required_sandbox_rejects_env_wrapped_shell_entries_before_execution() {
    let fixture = Fixture::new();
    fixture.write_executable("bwrap", "#!/bin/sh\nexit 0\n");
    let input = fixture.write_png_input("photo.png");
    fixture.write_thumbnailer("shell.thumbnailer", "env -u FOO sh -c true", "image/png;");

    let records = fixture.run_jsonl_code(
        [
            "--dry-run",
            "--size",
            "normal",
            "--format",
            "jsonl",
            input.to_str().unwrap(),
        ],
        1,
    );

    assert_eq!(records[0]["decision"], "failed");
    assert_eq!(records[0]["reason"], "sandbox-ineligible");
    assert_eq!(
        records[0]["sandbox_eligibility"],
        "runtime-exposure-unavailable"
    );
}

#[cfg(unix)]
#[test]
fn required_sandbox_rejects_literal_user_host_paths_before_execution() {
    let fixture = Fixture::new();
    fixture.write_executable("bwrap", "#!/bin/sh\nexit 0\n");
    let input = fixture.write_png_input("photo.png");
    let helper = fixture.write_executable("user-helper", "#!/bin/sh\nexit 0\n");
    fixture.write_thumbnailer(
        "host-path.thumbnailer",
        &format!("true {} %i %o %s", helper.display()),
        "image/png;",
    );

    let records = fixture.run_jsonl_code(
        [
            "--dry-run",
            "--size",
            "normal",
            "--format",
            "jsonl",
            input.to_str().unwrap(),
        ],
        1,
    );

    assert_eq!(records[0]["decision"], "failed");
    assert_eq!(records[0]["reason"], "sandbox-ineligible");
    assert!(
        records[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("literal host path")
    );
}

#[cfg(unix)]
#[test]
fn required_sandbox_rejects_env_wrapped_user_commands_before_execution() {
    let fixture = Fixture::new();
    fixture.write_executable("bwrap", "#!/bin/sh\nexit 0\n");
    let input = fixture.write_png_input("photo.png");
    fixture.write_executable("user-helper", "#!/bin/sh\nexit 0\n");
    fixture.write_thumbnailer(
        "env-user.thumbnailer",
        "env user-helper %i %o %s",
        "image/png;",
    );

    let records = fixture.run_jsonl_code(
        [
            "--dry-run",
            "--size",
            "normal",
            "--format",
            "jsonl",
            input.to_str().unwrap(),
        ],
        1,
    );

    assert_eq!(records[0]["decision"], "failed");
    assert_eq!(records[0]["reason"], "sandbox-ineligible");
    assert!(
        records[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("env-wrapped command")
    );
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
            .current_dir(self.root.path())
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
        parse_jsonl(output)
    }

    fn run_jsonl_code<const N: usize>(&self, args: [&str; N], code: i32) -> Vec<Value> {
        let output = self.command(args).code(code).get_output().stdout.clone();
        parse_jsonl(output)
    }

    fn write_png_input(&self, name: &str) -> std::path::PathBuf {
        let path = self.root.path().join(name);
        std::fs::write(&path, rendered_png()).unwrap();
        path
    }

    #[cfg(unix)]
    fn write_executable(&self, name: &str, content: &str) -> std::path::PathBuf {
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

    fn write_invalid_thumbnailer(&self, name: &str, mime_type: &str) {
        let dir = self.data_home.path().join("thumbnailers");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(name),
            format!("[Thumbnailer Entry]\nMimeType={mime_type}\n"),
        )
        .unwrap();
    }

    fn write_raw_thumbnailer(&self, name: &str, content: &str) {
        let dir = self.data_home.path().join("thumbnailers");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), content).unwrap();
    }

    fn install_existing_thumbnail(&self, input: &std::path::Path) {
        let metadata = std::fs::metadata(input).unwrap();
        let mtime = UnixMtimeSeconds::from_system_time(metadata.modified().unwrap()).unwrap();
        let uri =
            PersonalOriginalUri::from_absolute_path_bytes(input.as_os_str().as_encoded_bytes())
                .unwrap();
        let original = ReadablePersonalOriginalIdentity::assume_readable(
            PersonalOriginalIdentity::new(uri, mtime)
                .with_original_byte_size(metadata.len())
                .with_mime_type("image/png")
                .unwrap(),
        );
        PersonalCacheRoot::new(self.cache_home.path().join("thumbnails"))
            .unwrap()
            .install_thumbnail_returning_png_bytes(
                &original,
                ThumbnailSize::Normal,
                &rendered_png(),
            )
            .unwrap();
    }

    fn install_existing_thumbnail_without_optional_metadata(&self, input: &std::path::Path) {
        let metadata = std::fs::metadata(input).unwrap();
        let mtime = UnixMtimeSeconds::from_system_time(metadata.modified().unwrap()).unwrap();
        let uri =
            PersonalOriginalUri::from_absolute_path_bytes(input.as_os_str().as_encoded_bytes())
                .unwrap();
        let root = PersonalCacheRoot::new(self.cache_home.path().join("thumbnails")).unwrap();
        let path = root.cache_entry_path(&uri, &CacheNamespace::Size(ThumbnailSize::Normal));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            cache_png_with_metadata(BTreeMap::from([
                ("Thumb::URI", uri.as_str().to_owned()),
                ("Thumb::MTime", mtime.to_string()),
            ])),
        )
        .unwrap();
    }
}

fn assert_entry_uses_input_identity(fixture: &Fixture, entry: &Value, input: &std::path::Path) {
    let uri = PersonalOriginalUri::from_absolute_path_bytes(input.as_os_str().as_encoded_bytes())
        .unwrap();
    let cache_path = PersonalCacheRoot::new(fixture.cache_home.path().join("thumbnails"))
        .unwrap()
        .cache_entry_path(&uri, &CacheNamespace::Size(ThumbnailSize::Normal));

    assert_eq!(entry["input_path_display"], input.display().to_string());
    assert_eq!(entry["input_path_bytes_b64"], path_bytes_b64(input));
    assert_eq!(entry["uri"], uri.as_str());
    assert_eq!(entry["thumbnailer_uri"], uri.as_str());
    assert_eq!(
        entry["cache_path_display"],
        cache_path.display().to_string()
    );
    assert_eq!(entry["cache_path_bytes_b64"], path_bytes_b64(&cache_path));
}

fn path_bytes_b64(path: &std::path::Path) -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(path.as_os_str().as_encoded_bytes())
}

fn parse_jsonl(output: Vec<u8>) -> Vec<Value> {
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
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

fn cache_png_with_metadata(metadata: BTreeMap<&str, String>) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        for (key, value) in metadata {
            encoder.add_text_chunk(key.to_owned(), value).unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 8]).unwrap();
    }
    output
}
