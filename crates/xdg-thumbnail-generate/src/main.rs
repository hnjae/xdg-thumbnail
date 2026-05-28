// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};
use std::time::{Duration, Instant};

use base64::Engine;
use clap::{Parser, ValueEnum};
use serde::Serialize;
use xdg_thumbnail::{
    CacheNamespace, CacheRoot, PersonalThumbnailUri, ReadableOriginalIdentity, ThumbnailError,
    ThumbnailLookup, ThumbnailSize,
};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
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
    #[arg(long, value_enum, default_value_t = SandboxArg::Required)]
    sandbox: SandboxArg,
    #[arg(long, value_enum, default_value_t = FormatArg::Human)]
    format: FormatArg,
    #[arg(long)]
    verbose: bool,
    #[arg(required = true)]
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

#[derive(Clone, Debug)]
struct Thumbnailer {
    filename: String,
    path: PathBuf,
    exec: String,
    mime_types: Vec<String>,
    from_user_dir: bool,
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

#[derive(Serialize)]
struct ErrorRecord {
    kind: &'static str,
    message: String,
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

    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(3)
        }
    }
}

fn run(cli: Cli) -> std::result::Result<u8, String> {
    let root = CacheRoot::resolve_from_env().map_err(|error| error.to_string())?;
    let thumbnailers = discover_thumbnailers();
    let sizes = if cli.size.is_empty() {
        vec![ThumbnailSize::Normal]
    } else {
        cli.size.iter().copied().map(ThumbnailSize::from).collect()
    };
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let mime_db = xdg_mime::SharedMimeInfo::new();

    let mut summary = Summary::default();
    let mut records = Vec::new();
    for input in &cli.paths {
        let path = resolve_input_path(&cwd, input);
        for &size in &sizes {
            summary.requested += 1;
            records.push(plan_one(
                &cli,
                &root,
                &thumbnailers,
                &mime_db,
                &path,
                size,
                &mut summary,
            ));
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
        FormatArg::Human => write_human(&records, &summary, exit_code),
        FormatArg::Jsonl => write_jsonl(&records, &summary, exit_code),
    }

    Ok(exit_code)
}

fn plan_one(
    cli: &Cli,
    root: &CacheRoot,
    thumbnailers: &[Thumbnailer],
    mime_db: &xdg_mime::SharedMimeInfo,
    path: &Path,
    size: ThumbnailSize,
    summary: &mut Summary,
) -> EntryRecord {
    let mut record = base_record(cli, path, size);
    if is_recursive_input(root, path) {
        record.decision = "skip";
        record.reason = "unsupported-input";
        summary.skipped += 1;
        return record;
    }

    let original = match readable_original_for_path(path, None::<String>) {
        Ok(original) => original,
        Err(error) => {
            record.decision = "skip";
            record.reason = "input-unreadable";
            record.error = Some(ErrorRecord {
                kind: "input-unreadable",
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

    let mime_type = detect_mime_type(mime_db, path);
    record.mime_type.clone_from(&mime_type);
    let original = if let Some(mime_type) = mime_type.as_deref() {
        match readable_original_for_path(path, Some(mime_type.to_owned())) {
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
    let Some(thumbnailer) = select_thumbnailer(thumbnailers, &mime_type) else {
        record.decision = "skip";
        record.reason = "no-matching-thumbnailer";
        summary.skipped += 1;
        return record;
    };
    record.thumbnailer = Some(thumbnailer.filename.clone());

    if cli.sandbox == SandboxArg::Required {
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
            &thumbnailer.exec,
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

fn write_human(records: &[EntryRecord], summary: &Summary, exit_code: u8) {
    for record in records {
        println!(
            "{} {} input={} mime={} thumbnailer={} reason={}",
            record.decision,
            record.namespace,
            record.input_path_display,
            record.mime_type.as_deref().unwrap_or(""),
            record.thumbnailer.as_deref().unwrap_or(""),
            record.reason
        );
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

fn write_jsonl(records: &[EntryRecord], summary: &Summary, exit_code: u8) {
    for record in records {
        println!(
            "{}",
            serde_json::to_string(record).expect("serialize entry")
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
        exit_code,
    };
    println!(
        "{}",
        serde_json::to_string(&summary).expect("serialize summary")
    );
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
    mime_type: Option<impl Into<String>>,
) -> xdg_thumbnail::Result<ReadableOriginalIdentity> {
    ReadableOriginalIdentity::from_local_path(path, mime_type)
}

#[cfg(not(unix))]
fn readable_original_for_path(
    _path: &Path,
    _mime_type: Option<impl Into<String>>,
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

fn discover_thumbnailers() -> Vec<Thumbnailer> {
    let mut dirs = Vec::new();
    if let Some(data_home) = valid_data_home() {
        dirs.push((data_home.join("thumbnailers"), true));
    }
    for dir in data_dirs() {
        dirs.push((dir.join("thumbnailers"), false));
    }

    let mut seen = HashSet::new();
    let mut thumbnailers = Vec::new();
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
            if let Some(thumbnailer) = parse_thumbnailer(path, filename, from_user_dir) {
                thumbnailers.push(thumbnailer);
            }
        }
    }
    thumbnailers
}

fn parse_thumbnailer(path: PathBuf, filename: String, from_user_dir: bool) -> Option<Thumbnailer> {
    let entry = freedesktop_entry_parser::parse_entry(&path).ok()?;
    let section = entry.section("Thumbnailer Entry")?;
    let exec = section.attr("Exec").first()?.to_owned();
    let mime = section.attr("MimeType").first()?.to_owned();
    let mime_types = mime
        .split(';')
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if mime_types.is_empty() {
        return None;
    }
    if let Some(try_exec) = section.attr("TryExec").first() {
        resolve_executable(try_exec)?;
    }
    Some(Thumbnailer {
        filename,
        path,
        exec,
        mime_types,
        from_user_dir,
    })
}

fn select_thumbnailer<'a>(
    thumbnailers: &'a [Thumbnailer],
    mime_type: &str,
) -> Option<&'a Thumbnailer> {
    thumbnailers.iter().find(|thumbnailer| {
        let _ = (
            &thumbnailer.path,
            &thumbnailer.exec,
            thumbnailer.from_user_dir,
        );
        thumbnailer
            .mime_types
            .iter()
            .any(|candidate| candidate == mime_type)
    })
}

struct ExecutionError {
    reason: &'static str,
    message: String,
}

fn execute_thumbnailer(
    cli: &Cli,
    root: &CacheRoot,
    thumbnailer: &Thumbnailer,
    original: &ReadableOriginalIdentity,
    input_path: &Path,
    size: ThumbnailSize,
) -> Result<(), ExecutionError> {
    let output_dir = tempfile::tempdir().map_err(|error| ExecutionError {
        reason: "thumbnailer-output-missing",
        message: error.to_string(),
    })?;
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
        &thumbnailer.exec,
        &exec_input_path,
        &exec_input_uri,
        &exec_output_path,
        size,
    )?;
    let (program, args) = argv.split_first().ok_or_else(|| ExecutionError {
        reason: "thumbnailer-entry-invalid",
        message: "thumbnailer Exec expanded to an empty command".to_owned(),
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

    let mut child = command.spawn().map_err(|error| ExecutionError {
        reason: if cli.sandbox == SandboxArg::Required {
            "sandbox-unavailable"
        } else {
            "thumbnailer-exit"
        },
        message: error.to_string(),
    })?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= cli.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ExecutionError {
                    reason: "thumbnailer-timeout",
                    message: "thumbnailer exceeded configured timeout".to_owned(),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                return Err(ExecutionError {
                    reason: "thumbnailer-exit",
                    message: error.to_string(),
                });
            }
        }
    };
    if !status.success() {
        return Err(ExecutionError {
            reason: "thumbnailer-exit",
            message: status.to_string(),
        });
    }
    let rendered = std::fs::read(&host_output_path).map_err(|error| {
        let reason = if error.kind() == std::io::ErrorKind::NotFound {
            "thumbnailer-output-missing"
        } else {
            "thumbnailer-output-unreadable"
        };
        ExecutionError {
            reason,
            message: error.to_string(),
        }
    })?;
    if xdg_thumbnail::ParsedThumbnailPng::parse(&rendered).is_err() {
        return Err(ExecutionError {
            reason: "output-invalid-png",
            message: "thumbnailer output is not a valid PNG".to_owned(),
        });
    }
    root.install_personal_thumbnail(original, size, &rendered)
        .map(|_| ())
        .map_err(|error| {
            let reason = match &error {
                ThumbnailError::Png(_) => "output-invalid-png",
                ThumbnailError::UnsupportedRenderedThumbnail(_) => "output-normalization-failed",
                _ => "cache-install-failed",
            };
            ExecutionError {
                reason,
                message: error.to_string(),
            }
        })
}

fn check_required_sandbox_eligibility(thumbnailer: &Thumbnailer) -> Result<(), ExecutionError> {
    if resolve_executable("bwrap").is_none() {
        return Err(ExecutionError {
            reason: "sandbox-unavailable",
            message: "default mode requires Linux bubblewrap support; rerun with --sandbox off only if you trust the thumbnailer".to_owned(),
        });
    }
    let program = thumbnailer_program(thumbnailer)?;
    if is_shell(&program) {
        return Err(ExecutionError {
            reason: "sandbox-ineligible",
            message: "shell-based thumbnailer entries are not eligible for the required sandbox"
                .to_owned(),
        });
    }
    if !is_system_runtime_path(&program) {
        return Err(ExecutionError {
            reason: "sandbox-ineligible",
            message: format!(
                "thumbnailer executable {} is outside the required sandbox runtime profile",
                program.display()
            ),
        });
    }
    Ok(())
}

fn thumbnailer_program(thumbnailer: &Thumbnailer) -> Result<PathBuf, ExecutionError> {
    let words = shell_words::split(&thumbnailer.exec).map_err(|error| ExecutionError {
        reason: "thumbnailer-entry-invalid",
        message: error.to_string(),
    })?;
    let program = words.first().ok_or_else(|| ExecutionError {
        reason: "thumbnailer-entry-invalid",
        message: "thumbnailer Exec is empty".to_owned(),
    })?;
    resolve_executable(program).ok_or_else(|| ExecutionError {
        reason: "thumbnailer-entry-invalid",
        message: format!("thumbnailer executable {program} was not found"),
    })
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
) -> Result<Vec<String>, ExecutionError> {
    let words = shell_words::split(exec).map_err(|error| ExecutionError {
        reason: "thumbnailer-entry-invalid",
        message: error.to_string(),
    })?;
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
) -> Result<String, ExecutionError> {
    let mut output = String::new();
    let mut chars = word.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            output.push(ch);
            continue;
        }
        let Some(code) = chars.next() else {
            return Err(ExecutionError {
                reason: "thumbnailer-entry-invalid",
                message: "dangling percent field code".to_owned(),
            });
        };
        match code {
            'i' => output.push_str(&input_path.display().to_string()),
            'u' => output.push_str(input_uri),
            'o' => output.push_str(&output_path.display().to_string()),
            's' => output.push_str(&size.max_dimension().to_string()),
            '%' => output.push('%'),
            _ => {
                return Err(ExecutionError {
                    reason: "thumbnailer-entry-invalid",
                    message: format!("unknown thumbnailer field code %{code}"),
                });
            }
        }
    }
    Ok(output)
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
