// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};
use std::time::{Duration, Instant};

use base64::Engine;
use clap::{CommandFactory, Parser, ValueEnum};
use serde::Serialize;
use xdg_thumbnail::{
    CacheNamespace, CacheRoot, PersonalThumbnailUri, ReadableOriginalIdentity, ThumbnailError,
    ThumbnailLookup, ThumbnailSize,
};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    #[arg(long, value_enum)]
    size: Vec<SizeArg>,
    #[arg(long)]
    force: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, default_value = "30s", value_parser = parse_duration)]
    timeout: Duration,
    #[arg(long, value_enum, default_value_t = SandboxArg::Required, help = SANDBOX_HELP)]
    sandbox: SandboxArg,
    #[arg(long, value_enum, default_value_t = FormatArg::Human)]
    format: FormatArg,
    #[arg(long)]
    verbose: bool,
    #[arg(
        long,
        value_enum,
        value_name = "SHELL",
        conflicts_with = "generate_manpage"
    )]
    generate_completion: Option<clap_complete::Shell>,
    #[arg(long, conflicts_with = "generate_completion")]
    generate_manpage: bool,
    #[arg(required_unless_present_any = ["generate_completion", "generate_manpage"])]
    paths: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SizeArg {
    Normal,
    Large,
    #[value(name = "x-large")]
    XLarge,
    #[value(name = "xx-large")]
    XxLarge,
}

impl From<SizeArg> for ThumbnailSize {
    fn from(value: SizeArg) -> Self {
        match value {
            SizeArg::Normal => Self::Normal,
            SizeArg::Large => Self::Large,
            SizeArg::XLarge => Self::XLarge,
            SizeArg::XxLarge => Self::XxLarge,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SandboxArg {
    Required,
    Off,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum FormatArg {
    Human,
    Jsonl,
}

#[derive(Default)]
struct Summary {
    requested: u64,
    generated: u64,
    kept: u64,
    skipped: u64,
    failed: u64,
    warnings: u64,
}

const SANDBOX_HELP: &str = "Sandbox mode. The default requires Linux bubblewrap support and never falls back to unsandboxed execution; use --sandbox off only if you intentionally trust the selected thumbnailer.";
const SANDBOX_REQUIREMENT: &str = "default sandbox mode requires Linux bubblewrap support and never falls back to unsandboxed execution; use --sandbox off only if you intentionally trust the selected thumbnailer";

#[derive(Clone, Debug)]
struct Thumbnailer {
    filename: String,
    path: PathBuf,
    exec: Option<String>,
    mime_types: Vec<String>,
    from_user_dir: bool,
    invalid_message: Option<String>,
}

struct Discovery {
    thumbnailers: Vec<Thumbnailer>,
    warnings: Vec<WarningRecord>,
}

#[derive(Serialize)]
struct EntryRecord {
    schema_version: u8,
    event: &'static str,
    input_path_display: String,
    input_path_bytes_b64: Option<String>,
    uri: Option<String>,
    thumbnailer_uri: Option<String>,
    mime_type: Option<String>,
    thumbnailer: Option<String>,
    sandbox_mode: &'static str,
    sandbox_applied: bool,
    sandbox_eligibility: &'static str,
    namespace: String,
    cache_path_display: Option<String>,
    cache_path_bytes_b64: Option<String>,
    decision: &'static str,
    applied: bool,
    reason: &'static str,
    error: Option<ErrorRecord>,
}

#[derive(Clone, Serialize)]
struct ErrorRecord {
    kind: &'static str,
    message: String,
}

#[derive(Serialize)]
struct WarningRecord {
    schema_version: u8,
    event: &'static str,
    input_path_display: Option<String>,
    input_path_bytes_b64: Option<String>,
    mime_type: Option<String>,
    thumbnailer: String,
    reason: &'static str,
    error: ErrorRecord,
}

struct PlanContext<'a> {
    cli: &'a Cli,
    root: &'a CacheRoot,
    thumbnailers: &'a [Thumbnailer],
    mime_db: &'a xdg_mime::SharedMimeInfo,
    sandbox_backend_error: Option<&'a ExecutionError>,
}

#[derive(Serialize)]
struct SummaryRecord {
    schema_version: u8,
    event: &'static str,
    requested: u64,
    generated: u64,
    kept: u64,
    skipped: u64,
    failed: u64,
    warnings: u64,
    sandbox_requirement: Option<&'static str>,
    exit_code: u8,
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(2);
        }
    };

    match emit_generated_artifact(&cli) {
        Ok(Some(code)) => return code,
        Ok(None) => {}
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(3);
        }
    }

    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(3)
        }
    }
}

fn emit_generated_artifact(cli: &Cli) -> std::result::Result<Option<ExitCode>, String> {
    if let Some(shell) = cli.generate_completion {
        let mut command = Cli::command();
        let command_name = command.get_name().to_owned();
        let mut stdout = std::io::stdout().lock();
        clap_complete::generate(shell, &mut command, command_name, &mut stdout);
        return Ok(Some(ExitCode::SUCCESS));
    }

    if cli.generate_manpage {
        let mut stdout = std::io::stdout().lock();
        clap_mangen::Man::new(Cli::command())
            .render(&mut stdout)
            .map_err(|error| error.to_string())?;
        stdout.flush().map_err(|error| error.to_string())?;
        return Ok(Some(ExitCode::SUCCESS));
    }

    Ok(None)
}

fn run(cli: Cli) -> std::result::Result<u8, String> {
    let root = CacheRoot::resolve_from_env().map_err(|error| error.to_string())?;
    let discovery = discover_thumbnailers();
    let thumbnailers = discovery.thumbnailers;
    let sandbox_backend_error = if cli.sandbox == SandboxArg::Required {
        check_required_sandbox_backend().err()
    } else {
        None
    };
    let sizes = if cli.size.is_empty() {
        vec![ThumbnailSize::Normal]
    } else {
        cli.size.iter().copied().map(ThumbnailSize::from).collect()
    };
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let mime_db = xdg_mime::SharedMimeInfo::new();

    let mut summary = Summary::default();
    let mut records = Vec::new();
    let mut warnings = discovery.warnings;
    summary.warnings = warnings.len() as u64;
    for input in &cli.paths {
        let path = resolve_input_path(&cwd, input);
        let context = PlanContext {
            cli: &cli,
            root: &root,
            thumbnailers: &thumbnailers,
            mime_db: &mime_db,
            sandbox_backend_error: sandbox_backend_error.as_ref(),
        };
        for &size in &sizes {
            summary.requested += 1;
            records.push(plan_one(&context, &path, size, &mut summary, &mut warnings));
        }
    }

    let exit_code = if summary.failed > 0 {
        1
    } else if summary.skipped > 0 {
        4
    } else {
        0
    };

    match cli.format {
        FormatArg::Human => write_human(&records, &warnings, &summary, exit_code),
        FormatArg::Jsonl => write_jsonl(&records, &warnings, &summary, exit_code),
    }

    Ok(exit_code)
}

fn plan_one(
    context: &PlanContext<'_>,
    path: &Path,
    size: ThumbnailSize,
    summary: &mut Summary,
    warnings: &mut Vec<WarningRecord>,
) -> EntryRecord {
    let cli = context.cli;
    let root = context.root;
    let mut record = base_record(cli, path, size);
    if is_recursive_input(root, path) {
        record.decision = "skip";
        record.reason = "unsupported-input";
        summary.skipped += 1;
        return record;
    }

    let original = match readable_original_for_path(path, None) {
        Ok(original) => original,
        Err(error) => {
            let reason = original_error_reason(&error);
            record.decision = "skip";
            record.reason = reason;
            record.error = Some(ErrorRecord {
                kind: reason,
                message: error.to_string(),
            });
            summary.skipped += 1;
            return record;
        }
    };
    record.uri = Some(original.identity().uri().as_str().to_owned());
    let cache_path = root.personal_path(original.identity().uri(), &CacheNamespace::Size(size));
    record.cache_path_display = Some(cache_path.display().to_string());
    record.cache_path_bytes_b64 = path_bytes_b64(&cache_path);

    let mime_type = detect_mime_type(context.mime_db, path);
    record.mime_type.clone_from(&mime_type);
    let original = if let Some(mime_type) = mime_type.as_deref() {
        match readable_original_for_path(path, Some(mime_type)) {
            Ok(original) => original,
            Err(_) => original,
        }
    } else {
        original
    };

    if !cli.force {
        match root.validated_personal_path(&original, size) {
            Ok(ThumbnailLookup::Valid(_)) => {
                record.decision = "keep";
                record.reason = "already-valid";
                summary.kept += 1;
                return record;
            }
            Ok(
                ThumbnailLookup::Missing
                | ThumbnailLookup::Invalid(_)
                | ThumbnailLookup::Unverifiable(_),
            ) => {}
            Err(error) => {
                record.decision = "skip";
                record.reason = "cache-install-failed";
                record.error = Some(ErrorRecord {
                    kind: "cache-lookup-failed",
                    message: error.to_string(),
                });
                summary.skipped += 1;
                return record;
            }
        }
    }

    let Some(mime_type) = mime_type else {
        record.decision = "skip";
        record.reason = "mime-unknown";
        summary.skipped += 1;
        return record;
    };
    let matching_invalid =
        matching_invalid_thumbnailers(context.thumbnailers, context.mime_db, &mime_type);
    let Some(thumbnailer) = select_thumbnailer(context.thumbnailers, context.mime_db, &mime_type)
    else {
        if let Some(invalid) = matching_invalid.first() {
            record.decision = "skip";
            record.reason = "thumbnailer-entry-invalid";
            record.thumbnailer = Some(invalid.filename.clone());
            record.error = Some(ErrorRecord {
                kind: "thumbnailer-entry-invalid",
                message: invalid
                    .invalid_message
                    .clone()
                    .unwrap_or_else(|| "thumbnailer entry is invalid".to_owned()),
            });
        } else {
            record.decision = "skip";
            record.reason = "no-matching-thumbnailer";
        }
        summary.skipped += 1;
        return record;
    };
    record.thumbnailer = Some(thumbnailer.filename.clone());
    for invalid in matching_invalid {
        warnings.push(WarningRecord {
            schema_version: 0,
            event: "warning",
            input_path_display: Some(path.display().to_string()),
            input_path_bytes_b64: path_bytes_b64(path),
            mime_type: Some(mime_type.clone()),
            thumbnailer: invalid.filename.clone(),
            reason: "thumbnailer-entry-invalid",
            error: ErrorRecord {
                kind: "thumbnailer-entry-invalid",
                message: invalid
                    .invalid_message
                    .clone()
                    .unwrap_or_else(|| "thumbnailer entry is invalid".to_owned()),
            },
        });
        summary.warnings += 1;
    }

    if cli.sandbox == SandboxArg::Required {
        if let Some(error) = context.sandbox_backend_error {
            record.decision = "failed";
            record.reason = error.reason;
            record.sandbox_eligibility = "backend-unavailable";
            record.error = Some(ErrorRecord {
                kind: error.reason,
                message: error.message.clone(),
            });
            summary.failed += 1;
            return record;
        }
        if let Err(error) = check_required_sandbox_eligibility(thumbnailer) {
            record.decision = "failed";
            record.reason = error.reason;
            record.sandbox_eligibility = if error.reason == "sandbox-ineligible" {
                "runtime-exposure-unavailable"
            } else {
                "backend-unavailable"
            };
            record.error = Some(ErrorRecord {
                kind: error.reason,
                message: error.message,
            });
            summary.failed += 1;
            return record;
        }
    }

    record.thumbnailer_uri = Some(thumbnailer_uri(
        cli.sandbox,
        original.identity().uri().as_str(),
    ));
    if cli.dry_run {
        let dry_run_output = if cli.sandbox == SandboxArg::Required {
            PathBuf::from("/tmp/thumbnail.png")
        } else {
            std::env::temp_dir().join("xdg-thumbnail-dry-run-output.png")
        };
        let dry_run_input = if cli.sandbox == SandboxArg::Required {
            Path::new("/run/xdg-thumbnail/input")
        } else {
            path
        };
        if let Err(error) = expand_exec(
            selected_thumbnailer_exec(thumbnailer),
            dry_run_input,
            record.thumbnailer_uri.as_deref().unwrap_or_default(),
            &dry_run_output,
            size,
        ) {
            record.decision = "skip";
            record.reason = error.reason;
            record.error = Some(ErrorRecord {
                kind: error.reason,
                message: error.message,
            });
            summary.skipped += 1;
            return record;
        }
        record.decision = "generate";
        record.reason = "dry-run";
        summary.generated += 1;
        return record;
    }

    match execute_thumbnailer(cli, root, thumbnailer, &original, path, size) {
        Ok(()) => {
            record.decision = "generated";
            record.reason = "created";
            record.applied = true;
            record.sandbox_applied = cli.sandbox == SandboxArg::Required;
            summary.generated += 1;
        }
        Err(error) => {
            record.sandbox_applied = error.sandbox_applied;
            record.reason = error.reason;
            record.error = Some(ErrorRecord {
                kind: error.reason,
                message: error.message,
            });
            if error.reason == "thumbnailer-entry-invalid" {
                record.decision = "skip";
                summary.skipped += 1;
            } else {
                record.decision = "failed";
                summary.failed += 1;
            }
        }
    }
    record
}

fn base_record(cli: &Cli, path: &Path, size: ThumbnailSize) -> EntryRecord {
    EntryRecord {
        schema_version: 0,
        event: "entry",
        input_path_display: path.display().to_string(),
        input_path_bytes_b64: path_bytes_b64(path),
        uri: None,
        thumbnailer_uri: None,
        mime_type: None,
        thumbnailer: None,
        sandbox_mode: sandbox_name(cli.sandbox),
        sandbox_applied: false,
        sandbox_eligibility: "eligible",
        namespace: size.directory_name().to_owned(),
        cache_path_display: None,
        cache_path_bytes_b64: None,
        decision: "skip",
        applied: false,
        reason: "unsupported-input",
        error: None,
    }
}

fn write_human(
    records: &[EntryRecord],
    warnings: &[WarningRecord],
    summary: &Summary,
    exit_code: u8,
) {
    for record in records {
        println!(
            "{} {} input={} uri={} thumbnailer-uri={} mime={} thumbnailer={} sandbox={} sandbox-applied={} cache={} applied={} reason={}",
            record.decision,
            record.namespace,
            record.input_path_display,
            record.uri.as_deref().unwrap_or(""),
            record.thumbnailer_uri.as_deref().unwrap_or(""),
            record.mime_type.as_deref().unwrap_or(""),
            record.thumbnailer.as_deref().unwrap_or(""),
            record.sandbox_mode,
            record.sandbox_applied,
            record.cache_path_display.as_deref().unwrap_or(""),
            record.applied,
            record.reason
        );
        if let Some(error) = &record.error {
            println!("error kind={} message={}", error.kind, error.message);
        }
    }
    for warning in warnings {
        println!(
            "warning input={} mime={} thumbnailer={} reason={} message={}",
            warning.input_path_display.as_deref().unwrap_or(""),
            warning.mime_type.as_deref().unwrap_or(""),
            warning.thumbnailer,
            warning.reason,
            warning.error.message
        );
    }
    if needs_sandbox_requirement(records) {
        println!("sandbox requirement: {SANDBOX_REQUIREMENT}");
    }
    println!(
        "summary requested={} generated={} kept={} skipped={} failed={} warnings={} exit-code={}",
        summary.requested,
        summary.generated,
        summary.kept,
        summary.skipped,
        summary.failed,
        summary.warnings,
        exit_code
    );
}

fn write_jsonl(
    records: &[EntryRecord],
    warnings: &[WarningRecord],
    summary: &Summary,
    exit_code: u8,
) {
    for record in records {
        println!(
            "{}",
            serde_json::to_string(record).expect("serialize entry")
        );
    }
    for warning in warnings {
        println!(
            "{}",
            serde_json::to_string(warning).expect("serialize warning")
        );
    }
    let summary = SummaryRecord {
        schema_version: 0,
        event: "summary",
        requested: summary.requested,
        generated: summary.generated,
        kept: summary.kept,
        skipped: summary.skipped,
        failed: summary.failed,
        warnings: summary.warnings,
        sandbox_requirement: needs_sandbox_requirement(records).then_some(SANDBOX_REQUIREMENT),
        exit_code,
    };
    println!(
        "{}",
        serde_json::to_string(&summary).expect("serialize summary")
    );
}

fn needs_sandbox_requirement(records: &[EntryRecord]) -> bool {
    records
        .iter()
        .any(|record| matches!(record.reason, "sandbox-unavailable" | "sandbox-ineligible"))
}

fn resolve_input_path(cwd: &Path, input: &Path) -> PathBuf {
    if input.is_absolute() {
        input.to_owned()
    } else {
        cwd.join(input)
    }
}

fn is_recursive_input(root: &CacheRoot, path: &Path) -> bool {
    path.starts_with(root.as_path())
        || path.components().any(
            |component| matches!(component, Component::Normal(name) if name == ".sh_thumbnails"),
        )
}

#[cfg(unix)]
fn readable_original_for_path(
    path: &Path,
    mime_type: Option<&str>,
) -> xdg_thumbnail::Result<ReadableOriginalIdentity> {
    if let Some(mime_type) = mime_type {
        ReadableOriginalIdentity::from_local_path_with_mime_type(path, mime_type)
    } else {
        ReadableOriginalIdentity::from_local_path(path)
    }
}

#[cfg(not(unix))]
fn readable_original_for_path(
    _path: &Path,
    _mime_type: Option<&str>,
) -> xdg_thumbnail::Result<ReadableOriginalIdentity> {
    Err(xdg_thumbnail::ThumbnailError::UnsupportedPlatform)
}

fn detect_mime_type(mime_db: &xdg_mime::SharedMimeInfo, path: &Path) -> Option<String> {
    if path.extension().is_some_and(|extension| extension == "png") {
        return Some("image/png".to_owned());
    }
    let mut guess = mime_db.guess_mime_type();
    let guess = guess.path(path).guess();
    let mime = guess.mime_type().to_string();
    if guess.uncertain() && mime == "application/octet-stream" {
        None
    } else {
        Some(mime)
    }
}

fn original_error_reason(error: &ThumbnailError) -> &'static str {
    match error {
        ThumbnailError::UnsupportedPlatform => "unsupported-input",
        ThumbnailError::InvalidUriIdentity(_) => "uri-construction-failed",
        ThumbnailError::InvalidMetadata(_) => "original-metadata-unavailable",
        ThumbnailError::Io {
            context: "open original for reading",
            ..
        } => "input-unreadable",
        ThumbnailError::Io {
            context: "read original metadata" | "read original modification time",
            ..
        } => "original-metadata-unavailable",
        ThumbnailError::Io { .. } => "input-unreadable",
        _ => "input-unreadable",
    }
}

fn installation_error_reason(error: &ThumbnailError) -> &'static str {
    match error {
        ThumbnailError::UnsupportedRenderedThumbnail(message) => {
            if message.contains("unsupported") || message.contains("animated") {
                "output-unsupported-png"
            } else {
                "output-normalization-failed"
            }
        }
        ThumbnailError::Png(_) => "output-normalization-failed",
        ThumbnailError::InvalidMetadata(_) => "metadata-write-failed",
        ThumbnailError::InsecureCacheDirectory(_) => "permission-setup-failed",
        ThumbnailError::Io {
            context:
                "create parent thumbnail cache directories"
                | "create thumbnail cache directory"
                | "set thumbnail cache directory permissions"
                | "create thumbnail temporary file"
                | "set thumbnail temporary file permissions",
            ..
        } => "permission-setup-failed",
        ThumbnailError::Io { .. } => "cache-install-failed",
        _ => "cache-install-failed",
    }
}

fn discover_thumbnailers() -> Discovery {
    let mut dirs = Vec::new();
    if let Some(data_home) = valid_data_home() {
        dirs.push((data_home.join("thumbnailers"), true));
    }
    for dir in data_dirs() {
        dirs.push((dir.join("thumbnailers"), false));
    }

    let mut seen = HashSet::new();
    let mut thumbnailers = Vec::new();
    let mut warnings = Vec::new();
    for (dir, from_user_dir) in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "thumbnailer"))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let Some(filename) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
            else {
                continue;
            };
            if !seen.insert(filename.clone()) {
                continue;
            }
            match parse_thumbnailer(path, filename.clone(), from_user_dir) {
                Ok(Some(thumbnailer)) => thumbnailers.push(thumbnailer),
                Ok(None) => {}
                Err(message) => warnings.push(WarningRecord {
                    schema_version: 0,
                    event: "warning",
                    input_path_display: None,
                    input_path_bytes_b64: None,
                    mime_type: None,
                    thumbnailer: filename,
                    reason: "thumbnailer-entry-invalid",
                    error: ErrorRecord {
                        kind: "thumbnailer-entry-invalid",
                        message,
                    },
                }),
            }
        }
    }
    Discovery {
        thumbnailers,
        warnings,
    }
}

fn parse_thumbnailer(
    path: PathBuf,
    filename: String,
    from_user_dir: bool,
) -> Result<Option<Thumbnailer>, String> {
    let entry = freedesktop_entry_parser::parse_entry(&path).map_err(|error| error.to_string())?;
    let section = entry
        .section("Thumbnailer Entry")
        .ok_or_else(|| "thumbnailer entry is missing [Thumbnailer Entry]".to_owned())?;
    let mime = section
        .attr("MimeType")
        .first()
        .ok_or_else(|| "thumbnailer entry is missing MimeType".to_owned())?
        .to_owned();
    let mime_types = mime
        .split(';')
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if mime_types.is_empty() {
        return Err("thumbnailer entry has empty MimeType".to_owned());
    }
    if let Some(try_exec) = section.attr("TryExec").first() {
        if resolve_executable(try_exec).is_none() {
            return Ok(None);
        }
    }
    let exec = section.attr("Exec").first().map(ToOwned::to_owned);
    let invalid_message = match exec.as_deref() {
        Some(exec) => validate_thumbnailer_exec_template(exec).err(),
        None => Some("thumbnailer entry is missing Exec".to_owned()),
    };
    Ok(Some(Thumbnailer {
        filename,
        path,
        exec,
        mime_types,
        from_user_dir,
        invalid_message,
    }))
}

fn select_thumbnailer<'a>(
    thumbnailers: &'a [Thumbnailer],
    mime_db: &xdg_mime::SharedMimeInfo,
    mime_type: &str,
) -> Option<&'a Thumbnailer> {
    thumbnailers.iter().find(|thumbnailer| {
        let _ = (
            &thumbnailer.path,
            &thumbnailer.exec,
            thumbnailer.from_user_dir,
        );
        thumbnailer.invalid_message.is_none()
            && thumbnailer.exec.is_some()
            && thumbnailer_matches_mime(thumbnailer, mime_db, mime_type)
    })
}

fn matching_invalid_thumbnailers<'a>(
    thumbnailers: &'a [Thumbnailer],
    mime_db: &xdg_mime::SharedMimeInfo,
    mime_type: &str,
) -> Vec<&'a Thumbnailer> {
    thumbnailers
        .iter()
        .filter(|thumbnailer| {
            (thumbnailer.invalid_message.is_some() || thumbnailer.exec.is_none())
                && thumbnailer_matches_mime(thumbnailer, mime_db, mime_type)
        })
        .collect()
}

fn thumbnailer_matches_mime(
    thumbnailer: &Thumbnailer,
    mime_db: &xdg_mime::SharedMimeInfo,
    mime_type: &str,
) -> bool {
    let Ok(detected) = mime_type.parse::<mime::Mime>() else {
        return false;
    };
    thumbnailer.mime_types.iter().any(|candidate| {
        let Ok(candidate) = candidate.parse::<mime::Mime>() else {
            return false;
        };
        mime_db.mime_type_equal(&detected, &candidate)
            || mime_db.mime_type_subclass(&detected, &candidate)
    })
}

fn selected_thumbnailer_exec(thumbnailer: &Thumbnailer) -> &str {
    thumbnailer
        .exec
        .as_deref()
        .expect("selected thumbnailer has a valid Exec")
}

fn validate_thumbnailer_exec_template(exec: &str) -> Result<(), String> {
    let words = shell_words::split(exec).map_err(|error| error.to_string())?;
    if words.is_empty() {
        return Err("thumbnailer Exec is empty".to_owned());
    }
    for word in words {
        validate_field_codes(&word)?;
    }
    Ok(())
}

fn validate_field_codes(word: &str) -> Result<(), String> {
    let bytes = word.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        index += 1;
        let Some(&code) = bytes.get(index) else {
            return Err("dangling percent field code".to_owned());
        };
        if !matches!(code, b'i' | b'u' | b'o' | b's' | b'%') {
            return Err(format!(
                "unknown thumbnailer field code %{}",
                char::from(code)
            ));
        }
        index += 1;
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct ExecutionError {
    reason: &'static str,
    message: String,
    sandbox_applied: bool,
}

impl ExecutionError {
    fn new(reason: &'static str, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
            sandbox_applied: false,
        }
    }

    fn with_sandbox(
        reason: &'static str,
        message: impl Into<String>,
        sandbox_applied: bool,
    ) -> Self {
        Self {
            reason,
            message: message.into(),
            sandbox_applied,
        }
    }
}

fn execute_thumbnailer(
    cli: &Cli,
    root: &CacheRoot,
    thumbnailer: &Thumbnailer,
    original: &ReadableOriginalIdentity,
    input_path: &Path,
    size: ThumbnailSize,
) -> Result<(), ExecutionError> {
    let output_dir = tempfile::tempdir()
        .map_err(|error| ExecutionError::new("thumbnailer-output-missing", error.to_string()))?;
    let (exec_input_path, exec_input_uri, exec_output_path, host_output_path) =
        if cli.sandbox == SandboxArg::Required {
            (
                PathBuf::from("/run/xdg-thumbnail/input"),
                thumbnailer_uri(cli.sandbox, original.identity().uri().as_str()),
                PathBuf::from("/tmp/thumbnail.png"),
                output_dir.path().join("thumbnail.png"),
            )
        } else {
            (
                input_path.to_owned(),
                original.identity().uri().as_str().to_owned(),
                output_dir.path().join("thumbnail.png"),
                output_dir.path().join("thumbnail.png"),
            )
        };
    let argv = expand_exec(
        selected_thumbnailer_exec(thumbnailer),
        &exec_input_path,
        &exec_input_uri,
        &exec_output_path,
        size,
    )?;
    let (program, args) = argv.split_first().ok_or_else(|| {
        ExecutionError::new(
            "thumbnailer-entry-invalid",
            "thumbnailer Exec expanded to an empty command",
        )
    })?;

    let mut command = if cli.sandbox == SandboxArg::Required {
        let program_path = thumbnailer_program(thumbnailer)?;
        let mut command = ProcessCommand::new("bwrap");
        command
            .arg("--die-with-parent")
            .arg("--unshare-net")
            .arg("--new-session")
            .arg("--dev")
            .arg("/dev")
            .arg("--proc")
            .arg("/proc")
            .arg("--dir")
            .arg("/run")
            .arg("--dir")
            .arg("/run/xdg-thumbnail")
            .arg("--ro-bind")
            .arg(input_path)
            .arg("/run/xdg-thumbnail/input")
            .arg("--dir")
            .arg("/tmp")
            .arg("--bind")
            .arg(output_dir.path())
            .arg("/tmp");
        add_system_binds(&mut command);
        command.arg(program_path).args(args);
        command
    } else {
        let mut command = ProcessCommand::new(program);
        command.args(args);
        command
    };

    let mut child = command.spawn().map_err(|error| {
        ExecutionError::new(
            if cli.sandbox == SandboxArg::Required {
                "sandbox-unavailable"
            } else {
                "thumbnailer-exit"
            },
            error.to_string(),
        )
    })?;
    let sandbox_applied = cli.sandbox == SandboxArg::Required;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= cli.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ExecutionError::with_sandbox(
                    "thumbnailer-timeout",
                    "thumbnailer exceeded configured timeout",
                    sandbox_applied,
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                return Err(ExecutionError::with_sandbox(
                    "thumbnailer-exit",
                    error.to_string(),
                    sandbox_applied,
                ));
            }
        }
    };
    if !status.success() {
        return Err(ExecutionError::with_sandbox(
            "thumbnailer-exit",
            status.to_string(),
            sandbox_applied,
        ));
    }
    let rendered = std::fs::read(&host_output_path).map_err(|error| {
        let reason = if error.kind() == std::io::ErrorKind::NotFound {
            "thumbnailer-output-missing"
        } else {
            "thumbnailer-output-unreadable"
        };
        ExecutionError::with_sandbox(reason, error.to_string(), sandbox_applied)
    })?;
    if xdg_thumbnail::ParsedThumbnailPng::parse(&rendered).is_err() {
        return Err(ExecutionError::with_sandbox(
            "output-invalid-png",
            "thumbnailer output is not a valid PNG",
            sandbox_applied,
        ));
    }
    root.install_personal_thumbnail_path(original, size, &rendered)
        .map(|_| ())
        .map_err(|error| {
            ExecutionError::with_sandbox(
                installation_error_reason(&error),
                error.to_string(),
                sandbox_applied,
            )
        })
}

fn check_required_sandbox_eligibility(thumbnailer: &Thumbnailer) -> Result<(), ExecutionError> {
    let words = exec_words(selected_thumbnailer_exec(thumbnailer))?;
    let Some((_, args)) = words.split_first() else {
        return Err(ExecutionError::new(
            "thumbnailer-entry-invalid",
            "thumbnailer Exec is empty",
        ));
    };
    let program = thumbnailer_program_from_words(&words)?;
    if is_shell(&program) {
        return Err(sandbox_ineligible(
            "shell-based thumbnailer entries are not eligible for the required sandbox",
        ));
    }
    if let Some(command) = env_wrapped_command(&program, args) {
        if is_shell(&command) {
            return Err(sandbox_ineligible(
                "shell-based thumbnailer entries are not eligible for the required sandbox",
            ));
        }
        if !is_system_runtime_path(&command) {
            return Err(sandbox_ineligible(&format!(
                "thumbnailer env-wrapped command {} is outside the required sandbox runtime profile",
                command.display()
            )));
        }
        check_script_interpreter(&command)?;
    }
    if !is_system_runtime_path(&program) {
        return Err(sandbox_ineligible(&format!(
            "thumbnailer executable {} is outside the required sandbox runtime profile",
            program.display()
        )));
    }
    check_script_interpreter(&program)?;
    for literal_path in args.iter().filter_map(|word| literal_host_path(word)) {
        if !is_system_runtime_path(literal_path) {
            return Err(sandbox_ineligible(&format!(
                "thumbnailer literal host path {} is outside the required sandbox runtime profile",
                literal_path.display()
            )));
        }
    }
    Ok(())
}

fn check_required_sandbox_backend() -> Result<(), ExecutionError> {
    if !cfg!(target_os = "linux") {
        return Err(ExecutionError::new(
            "sandbox-unavailable",
            sandbox_message("default mode is unsupported on this platform"),
        ));
    }
    if resolve_executable("bwrap").is_none() {
        return Err(ExecutionError::new(
            "sandbox-unavailable",
            sandbox_message("bubblewrap was not found in PATH"),
        ));
    }
    let true_path = resolve_executable("true").ok_or_else(|| {
        ExecutionError::new(
            "sandbox-unavailable",
            sandbox_message("could not find a runtime helper for sandbox probing"),
        )
    })?;
    let mut command = ProcessCommand::new("bwrap");
    command
        .arg("--die-with-parent")
        .arg("--unshare-net")
        .arg("--new-session")
        .arg("--dev")
        .arg("/dev")
        .arg("--proc")
        .arg("/proc")
        .arg("--dir")
        .arg("/tmp");
    add_system_binds(&mut command);
    command.arg(true_path);
    let status = command.status().map_err(|error| {
        ExecutionError::new(
            "sandbox-unavailable",
            sandbox_message(&format!("bubblewrap probe could not start: {error}")),
        )
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(ExecutionError::new(
            "sandbox-unavailable",
            sandbox_message(&format!("bubblewrap probe failed: {status}")),
        ))
    }
}

fn sandbox_ineligible(detail: &str) -> ExecutionError {
    ExecutionError::new("sandbox-ineligible", sandbox_message(detail))
}

fn sandbox_message(detail: &str) -> String {
    format!(
        "{detail}; default mode requires Linux bubblewrap support, no unsandboxed fallback is attempted, and users who trust the selected thumbnailer may rerun with --sandbox off"
    )
}

fn check_script_interpreter(program: &Path) -> Result<(), ExecutionError> {
    let Some(words) = script_interpreter_words(program)? else {
        return Ok(());
    };
    let Some((interpreter, args)) = words.split_first() else {
        return Ok(());
    };
    let interpreter = resolve_executable(interpreter).ok_or_else(|| {
        sandbox_ineligible(&format!(
            "thumbnailer script interpreter {interpreter} was not found"
        ))
    })?;
    check_sandbox_runtime_path("thumbnailer script interpreter", &interpreter)?;
    if interpreter.file_name().and_then(|name| name.to_str()) == Some("env") {
        let command = env_wrapped_command(&interpreter, args).ok_or_else(|| {
            sandbox_ineligible("thumbnailer script env interpreter did not resolve a command")
        })?;
        check_sandbox_runtime_path("thumbnailer script env-wrapped command", &command)?;
    }
    Ok(())
}

fn script_interpreter_words(program: &Path) -> Result<Option<Vec<String>>, ExecutionError> {
    let mut file = File::open(program).map_err(|error| {
        sandbox_ineligible(&format!(
            "thumbnailer executable {} could not be inspected for a script interpreter: {error}",
            program.display()
        ))
    })?;
    let mut buffer = [0; 512];
    let bytes_read = file.read(&mut buffer).map_err(|error| {
        sandbox_ineligible(&format!(
            "thumbnailer executable {} could not be inspected for a script interpreter: {error}",
            program.display()
        ))
    })?;
    let buffer = &buffer[..bytes_read];
    if !buffer.starts_with(b"#!") {
        return Ok(None);
    }
    let line_end = buffer
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(buffer.len());
    let line = String::from_utf8_lossy(&buffer[2..line_end]);
    let words = shell_words::split(line.trim())
        .unwrap_or_else(|_| line.split_whitespace().map(ToOwned::to_owned).collect());
    Ok(Some(words))
}

fn check_sandbox_runtime_path(label: &str, path: &Path) -> Result<(), ExecutionError> {
    if is_shell(path) {
        return Err(sandbox_ineligible(
            "shell-based thumbnailer entries are not eligible for the required sandbox",
        ));
    }
    if !is_system_runtime_path(path) {
        return Err(sandbox_ineligible(&format!(
            "{label} {} is outside the required sandbox runtime profile",
            path.display()
        )));
    }
    Ok(())
}

fn thumbnailer_program(thumbnailer: &Thumbnailer) -> Result<PathBuf, ExecutionError> {
    let words = exec_words(selected_thumbnailer_exec(thumbnailer))?;
    thumbnailer_program_from_words(&words)
}

fn exec_words(exec: &str) -> Result<Vec<String>, ExecutionError> {
    shell_words::split(exec)
        .map_err(|error| ExecutionError::new("thumbnailer-entry-invalid", error.to_string()))
}

fn thumbnailer_program_from_words(words: &[String]) -> Result<PathBuf, ExecutionError> {
    let program = words.first().ok_or_else(|| {
        ExecutionError::new("thumbnailer-entry-invalid", "thumbnailer Exec is empty")
    })?;
    resolve_executable(program).ok_or_else(|| {
        ExecutionError::new(
            "thumbnailer-entry-invalid",
            format!("thumbnailer executable {program} was not found"),
        )
    })
}

fn env_wrapped_command(program: &Path, args: &[String]) -> Option<PathBuf> {
    if program.file_name().and_then(|name| name.to_str()) != Some("env") {
        return None;
    }
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-" || is_env_assignment(arg) {
            index += 1;
            continue;
        }
        if let Some(split_string) = arg.strip_prefix("--split-string=") {
            let split = shell_words::split(split_string).ok()?;
            return split
                .first()
                .and_then(|command| resolve_executable(command));
        }
        if arg == "-S" || arg == "--split-string" {
            let split = shell_words::split(args.get(index + 1)?).ok()?;
            return split
                .first()
                .and_then(|command| resolve_executable(command));
        }
        if arg == "-u" || arg == "--unset" || arg == "-C" || arg == "--chdir" {
            index += 2;
            continue;
        }
        if arg.starts_with("--unset=") || arg.starts_with("--chdir=") {
            index += 1;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        return resolve_executable(arg);
    }
    None
}

fn is_env_assignment(value: &str) -> bool {
    let Some((name, _)) = value.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn literal_host_path(word: &str) -> Option<&Path> {
    if word.contains('%') {
        return None;
    }
    if word.starts_with('/') {
        return Some(Path::new(word));
    }
    let (_, value) = word.split_once('=')?;
    value.starts_with('/').then(|| Path::new(value))
}

fn is_shell(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("sh" | "bash" | "dash" | "zsh" | "fish")
    )
}

fn is_system_runtime_path(path: &Path) -> bool {
    [
        Path::new("/usr"),
        Path::new("/bin"),
        Path::new("/sbin"),
        Path::new("/lib"),
        Path::new("/lib64"),
        Path::new("/nix/store"),
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
}

fn add_system_binds(command: &mut ProcessCommand) {
    for path in [
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/etc",
        "/nix/store",
    ] {
        let path = Path::new(path);
        if path.exists() {
            command.arg("--ro-bind").arg(path).arg(path);
        }
    }
}

fn thumbnailer_uri(sandbox: SandboxArg, host_uri: &str) -> String {
    if sandbox == SandboxArg::Required {
        PersonalThumbnailUri::from_absolute_path_bytes(b"/run/xdg-thumbnail/input")
            .map(|uri| uri.as_str().to_owned())
            .unwrap_or_else(|_| host_uri.to_owned())
    } else {
        host_uri.to_owned()
    }
}

fn expand_exec(
    exec: &str,
    input_path: &Path,
    input_uri: &str,
    output_path: &Path,
    size: ThumbnailSize,
) -> Result<Vec<OsString>, ExecutionError> {
    let words = shell_words::split(exec)
        .map_err(|error| ExecutionError::new("thumbnailer-entry-invalid", error.to_string()))?;
    words
        .into_iter()
        .map(|word| expand_field_codes(&word, input_path, input_uri, output_path, size))
        .collect()
}

fn expand_field_codes(
    word: &str,
    input_path: &Path,
    input_uri: &str,
    output_path: &Path,
    size: ThumbnailSize,
) -> Result<OsString, ExecutionError> {
    expand_field_codes_platform(word, input_path, input_uri, output_path, size)
}

#[cfg(unix)]
fn expand_field_codes_platform(
    word: &str,
    input_path: &Path,
    input_uri: &str,
    output_path: &Path,
    size: ThumbnailSize,
) -> Result<OsString, ExecutionError> {
    let mut output = Vec::new();
    let bytes = word.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        let Some(&code) = bytes.get(index) else {
            return Err(ExecutionError::new(
                "thumbnailer-entry-invalid",
                "dangling percent field code",
            ));
        };
        match code {
            b'i' => output.extend_from_slice(input_path.as_os_str().as_bytes()),
            b'u' => output.extend_from_slice(input_uri.as_bytes()),
            b'o' => output.extend_from_slice(output_path.as_os_str().as_bytes()),
            b's' => output.extend_from_slice(size.max_dimension().to_string().as_bytes()),
            b'%' => output.push(b'%'),
            _ => {
                return Err(ExecutionError::new(
                    "thumbnailer-entry-invalid",
                    format!("unknown thumbnailer field code %{}", char::from(code)),
                ));
            }
        }
        index += 1;
    }
    Ok(OsString::from_vec(output))
}

#[cfg(not(unix))]
fn expand_field_codes_platform(
    word: &str,
    input_path: &Path,
    input_uri: &str,
    output_path: &Path,
    size: ThumbnailSize,
) -> Result<OsString, ExecutionError> {
    let mut output = String::new();
    let mut chars = word.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            output.push(ch);
            continue;
        }
        let Some(code) = chars.next() else {
            return Err(ExecutionError::new(
                "thumbnailer-entry-invalid",
                "dangling percent field code",
            ));
        };
        match code {
            'i' => output.push_str(&input_path.display().to_string()),
            'u' => output.push_str(input_uri),
            'o' => output.push_str(&output_path.display().to_string()),
            's' => output.push_str(&size.max_dimension().to_string()),
            '%' => output.push('%'),
            _ => {
                return Err(ExecutionError::new(
                    "thumbnailer-entry-invalid",
                    format!("unknown thumbnailer field code %{code}"),
                ));
            }
        }
    }
    Ok(OsString::from(output))
}

fn valid_data_home() -> Option<PathBuf> {
    let value = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    match value {
        Some(path) if path.is_absolute() => Some(path),
        _ => std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|home| home.join(".local/share")),
    }
}

fn data_dirs() -> Vec<PathBuf> {
    let Some(value) = std::env::var_os("XDG_DATA_DIRS").filter(|value| !value.is_empty()) else {
        return vec![
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ];
    };
    std::env::split_paths(&value)
        .filter(|path| path.is_absolute())
        .collect()
}

fn resolve_executable(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.components().count() > 1 {
        return is_executable(path).then(|| path.to_owned());
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(value);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn sandbox_name(value: SandboxArg) -> &'static str {
    match value {
        SandboxArg::Required => "required",
        SandboxArg::Off => "off",
    }
}

#[cfg(unix)]
fn path_bytes_b64(path: &Path) -> Option<String> {
    Some(base64::engine::general_purpose::STANDARD_NO_PAD.encode(path.as_os_str().as_bytes()))
}

#[cfg(not(unix))]
fn path_bytes_b64(_path: &Path) -> Option<String> {
    None
}

fn parse_duration(input: &str) -> Result<Duration, String> {
    if input.len() < 2 {
        return Err("duration must include a unit suffix".to_owned());
    }
    let (number, unit) = input.split_at(input.len() - 1);
    let value = number
        .parse::<u64>()
        .map_err(|_| "duration must be a positive integer followed by s, m, h, or d".to_owned())?;
    if value == 0 {
        return Err("duration must be greater than zero".to_owned());
    }
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return Err("duration unit must be one of s, m, h, or d".to_owned()),
    };
    value
        .checked_mul(multiplier)
        .map(Duration::from_secs)
        .ok_or_else(|| "duration is too large".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn field_code_expansion_preserves_non_utf8_path_bytes() {
        let input = PathBuf::from(OsString::from_vec(b"/tmp/input-\xFF.png".to_vec()));
        let output = PathBuf::from(OsString::from_vec(b"/tmp/output-\xFE.png".to_vec()));

        let argv = expand_exec(
            "thumb %i %u %o %s",
            &input,
            "file:///tmp/input-%FF.png",
            &output,
            ThumbnailSize::Normal,
        )
        .unwrap();

        assert_eq!(argv[1].as_bytes(), b"/tmp/input-\xFF.png");
        assert_eq!(argv[2].as_bytes(), b"file:///tmp/input-%FF.png");
        assert_eq!(argv[3].as_bytes(), b"/tmp/output-\xFE.png");
        assert_eq!(argv[4].as_bytes(), b"128");
    }

    #[test]
    fn script_interpreter_words_parse_env_shebang() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), b"#!/usr/bin/env -S python3 -I\nprint('x')\n").unwrap();

        let words = script_interpreter_words(temp.path()).unwrap().unwrap();

        assert_eq!(words, vec!["/usr/bin/env", "-S", "python3", "-I"]);
    }
}
