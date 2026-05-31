// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use clap::{CommandFactory, Parser, ValueEnum};
use serde::Serialize;
use xdg_thumbnail::{
    AccessTimePreservation, CacheEntryInspection, CacheEntryInspectionOutcome, CacheEntryProblem,
    CacheNamespace, NonstandardEntryPolicy, OriginalUriIdentity, PersonalCacheRoot,
    PersonalOriginalUri, PersonalValidationOutcome, ReadablePersonalOriginalIdentity,
    ThumbnailError, ThumbnailMetadataProblemKind, ThumbnailSize, validate_personal_failure_entry,
    validate_personal_thumbnail,
};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    #[arg(long, default_value = "30d", value_parser = parse_duration)]
    older_than: Duration,
    #[arg(long)]
    delete: bool,
    #[arg(long)]
    delete_stale_local: bool,
    #[arg(long)]
    allow_delete_failures: bool,
    #[arg(long, value_enum)]
    size: Vec<SizeArg>,
    #[arg(long, value_enum, default_value_t = ScopeArg::Thumbnails)]
    scope: ScopeArg,
    #[arg(long)]
    include_nonstandard_files: bool,
    #[arg(long)]
    removable_prefix: Vec<PathBuf>,
    #[arg(long)]
    ignore_fhs_media: bool,
    #[arg(long, value_enum, default_value_t = AgeBasisArg::AccessTime)]
    age_basis: AgeBasisArg,
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
enum ScopeArg {
    Thumbnails,
    Failures,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum AgeBasisArg {
    #[value(name = "atime")]
    AccessTime,
    #[value(name = "mtime")]
    ModificationTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum FormatArg {
    Human,
    Jsonl,
}

#[derive(Default)]
struct Summary {
    scanned: u64,
    kept: u64,
    would_delete: u64,
    deleted: u64,
    skipped: u64,
    errors: u64,
    deletion_failed: bool,
    nonfatal_error: bool,
    timestamp_unavailable: u64,
    timestamp_unreliable: u64,
    timestamp_preservation_unavailable: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UriClass {
    LocalStableFile,
    LocalRemovableOrPortal,
    Remote,
    ArchiveOrVirtual,
    Unknown,
}

impl UriClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::LocalStableFile => "local-stable-file",
            Self::LocalRemovableOrPortal => "local-removable-or-portal",
            Self::Remote => "remote",
            Self::ArchiveOrVirtual => "archive-or-virtual",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decision {
    Keep,
    Delete,
    Stale,
    Skip,
}

impl Decision {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Delete => "delete",
            Self::Stale => "stale",
            Self::Skip => "skip",
        }
    }
}

#[derive(Serialize)]
struct EntryRecord {
    schema_version: u8,
    event: &'static str,
    thumbnail_path_display: String,
    thumbnail_path_bytes_b64: Option<String>,
    uri: Option<String>,
    namespace: String,
    classification: &'static str,
    decision: &'static str,
    applied: bool,
    reason: Option<&'static str>,
    age_basis: &'static str,
    timestamp: Option<i64>,
    access_time_preservation: &'static str,
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
    scanned: u64,
    kept: u64,
    would_delete: u64,
    deleted: u64,
    skipped: u64,
    errors: u64,
    age_basis: &'static str,
    timestamp_unavailable: u64,
    timestamp_unreliable: u64,
    timestamp_preservation_unavailable: u64,
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

    if cli.allow_delete_failures && matches!(cli.scope, ScopeArg::Thumbnails) {
        eprintln!("--allow-delete-failures requires --scope failures or --scope all");
        return ExitCode::from(2);
    }
    if !cli.size.is_empty() && matches!(cli.scope, ScopeArg::Failures) {
        eprintln!("--size cannot be used with --scope failures");
        return ExitCode::from(2);
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
    let root = PersonalCacheRoot::resolve_from_env().map_err(|error| error.to_string())?;
    let sizes = if cli.size.is_empty() {
        ThumbnailSize::all().to_vec()
    } else {
        cli.size.iter().copied().map(ThumbnailSize::from).collect()
    };
    let nonstandard_entry_policy = if cli.include_nonstandard_files {
        NonstandardEntryPolicy::Include
    } else {
        NonstandardEntryPolicy::Exclude
    };

    let mut entries = Vec::new();
    if matches!(cli.scope, ScopeArg::Thumbnails | ScopeArg::All) {
        entries.extend(
            root.inspect_thumbnails(&sizes, nonstandard_entry_policy)
                .map_err(|error| error.to_string())?,
        );
    }
    if matches!(cli.scope, ScopeArg::Failures | ScopeArg::All) {
        entries.extend(
            root.inspect_failure_entries(nonstandard_entry_policy)
                .map_err(|error| error.to_string())?,
        );
    }

    let classifier = Classifier::new(&cli);
    let mut summary = Summary::default();
    let mut records = Vec::new();
    for entry in entries {
        let record = evaluate_entry(&root, &cli, &classifier, entry, &mut summary);
        records.push(record);
    }

    match cli.format {
        FormatArg::Human => write_human(&records, &summary, cli.age_basis, cli.verbose),
        FormatArg::Jsonl => write_jsonl(&records, &summary, cli.age_basis),
    }

    if summary.deletion_failed {
        Ok(1)
    } else if summary.nonfatal_error {
        Ok(4)
    } else {
        Ok(0)
    }
}

fn evaluate_entry(
    _root: &PersonalCacheRoot,
    cli: &Cli,
    classifier: &Classifier,
    entry: CacheEntryInspection,
    summary: &mut Summary,
) -> EntryRecord {
    summary.scanned += 1;
    let thumbnail_path_display = entry.path().display().to_string();
    let thumbnail_path_bytes_b64 = path_bytes_b64(entry.path());
    let namespace = entry.namespace().to_string();
    let access_time_preservation =
        access_preservation_name(entry.timestamps().access_time_preserved_during_inspection());
    let uri = personal_uri(&entry).cloned();
    let uri_text = uri.as_ref().map(|uri| uri.as_str().to_owned());
    let classification = uri
        .as_ref()
        .map_or(UriClass::Unknown, |uri| classifier.classify(uri));
    let timestamp = selected_timestamp(&entry, cli.age_basis);
    let mut decision = Decision::Keep;
    let mut reason = None;
    let mut error = None;

    if let CacheEntryInspectionOutcome::Invalid(problems) = entry.outcome() {
        if problems.contains(&CacheEntryProblem::NonstandardFilename) {
            decision = Decision::Skip;
            reason = Some("nonstandard-filename");
        } else if only_nonconforming(problems) {
            if let Some(uri) = &uri {
                match classification {
                    UriClass::LocalStableFile => {
                        evaluate_local_file(
                            uri,
                            &entry,
                            cli,
                            &mut decision,
                            &mut reason,
                            &mut error,
                            summary,
                        );
                    }
                    UriClass::Remote
                    | UriClass::ArchiveOrVirtual
                    | UriClass::LocalRemovableOrPortal => {
                        evaluate_age_based(
                            &entry,
                            cli,
                            classification,
                            timestamp,
                            &mut decision,
                            &mut reason,
                            summary,
                        );
                    }
                    UriClass::Unknown => {
                        decision = Decision::Skip;
                        reason = first_nonconforming_reason(problems);
                    }
                }
            } else {
                decision = Decision::Skip;
                reason = first_nonconforming_reason(problems);
            }
        } else if problems.contains(&CacheEntryProblem::InvalidPngStructure) {
            decision = Decision::Delete;
            reason = Some("invalid-png-structure");
        } else if has_metadata_kind(problems, ThumbnailMetadataProblemKind::MissingRequired) {
            decision = Decision::Delete;
            reason = Some("missing-required-metadata");
        } else if has_metadata_kind(problems, ThumbnailMetadataProblemKind::InvalidSyntax) {
            decision = Decision::Delete;
            reason = Some("invalid-metadata-syntax");
        } else if problems.contains(&CacheEntryProblem::UriFilenameMismatch) {
            decision = Decision::Delete;
            reason = Some("uri-filename-mismatch");
        } else if problems.contains(&CacheEntryProblem::NonconformingPngFormat) {
            decision = Decision::Skip;
            reason = Some("nonconforming-format");
        } else if problems.contains(&CacheEntryProblem::DimensionsExceedNamespace) {
            decision = Decision::Skip;
            reason = Some("nonconforming-dimensions");
        } else if problems.contains(&CacheEntryProblem::ResourceLimitExceeded) {
            decision = Decision::Skip;
            reason = Some("resource-limit-exceeded");
        } else {
            decision = Decision::Skip;
            reason = Some("unreadable-entry");
            record_nonfatal_error(summary);
        }
    } else if let Some(uri) = &uri {
        match classification {
            UriClass::LocalStableFile => {
                evaluate_local_file(
                    uri,
                    &entry,
                    cli,
                    &mut decision,
                    &mut reason,
                    &mut error,
                    summary,
                );
            }
            UriClass::Remote | UriClass::ArchiveOrVirtual | UriClass::LocalRemovableOrPortal => {
                evaluate_age_based(
                    &entry,
                    cli,
                    classification,
                    timestamp,
                    &mut decision,
                    &mut reason,
                    summary,
                );
            }
            UriClass::Unknown => {
                decision = Decision::Skip;
                reason = Some("original-unverifiable");
            }
        }
    } else {
        decision = Decision::Skip;
        reason = Some("original-unverifiable");
    }

    if is_failure_namespace(entry.namespace())
        && decision == Decision::Delete
        && !cli.allow_delete_failures
    {
        decision = Decision::Skip;
        reason = Some("failure-deletion-not-enabled");
    }

    let mut applied = false;
    if decision == Decision::Delete {
        if cli.delete {
            match entry.into_handle().remove() {
                Ok(()) => {
                    applied = true;
                    summary.deleted += 1;
                }
                Err(remove_error) => {
                    summary.errors += 1;
                    summary.deletion_failed = true;
                    error = Some(ErrorRecord {
                        kind: "delete-failed",
                        message: remove_error.to_string(),
                    });
                }
            }
        } else {
            summary.would_delete += 1;
        }
    } else if decision == Decision::Skip {
        summary.skipped += 1;
    } else {
        summary.kept += 1;
    }

    EntryRecord {
        schema_version: 0,
        event: "entry",
        thumbnail_path_display,
        thumbnail_path_bytes_b64,
        uri: uri_text,
        namespace,
        classification: classification.as_str(),
        decision: decision.as_str(),
        applied,
        reason,
        age_basis: age_basis_name(cli.age_basis),
        timestamp: timestamp.and_then(system_time_seconds),
        access_time_preservation,
        error,
    }
}

fn evaluate_local_file(
    uri: &PersonalOriginalUri,
    entry: &CacheEntryInspection,
    cli: &Cli,
    decision: &mut Decision,
    reason: &mut Option<&'static str>,
    error: &mut Option<ErrorRecord>,
    summary: &mut Summary,
) {
    let Some(path) = local_file_uri_to_path(uri.as_str()) else {
        *decision = Decision::Skip;
        *reason = Some("original-unverifiable");
        return;
    };
    let original = match ReadablePersonalOriginalIdentity::from_local_path(&path) {
        Ok(original) => original,
        Err(read_error) if is_not_found_error(&read_error) => {
            *decision = Decision::Delete;
            *reason = Some("original-missing");
            return;
        }
        Err(read_error) => {
            *decision = Decision::Skip;
            *reason = Some("original-unverifiable");
            *error = Some(ErrorRecord {
                kind: "original-unverifiable",
                message: read_error.to_string(),
            });
            record_nonfatal_error(summary);
            return;
        }
    };
    let Ok(bytes) = std::fs::read(entry.path()) else {
        *decision = Decision::Skip;
        *reason = Some("unreadable-entry");
        record_nonfatal_error(summary);
        return;
    };
    let validation = if let Some(size) = successful_size(entry.namespace()) {
        validate_personal_thumbnail(&bytes, &original, size)
    } else {
        validate_personal_failure_entry(&bytes, &original)
    };
    match validation {
        PersonalValidationOutcome::FullyVerified => {
            *decision = Decision::Keep;
            *reason = None;
        }
        PersonalValidationOutcome::Invalid(problems)
            if has_metadata_kind(&problems, ThumbnailMetadataProblemKind::ValueMismatch) =>
        {
            if cli.delete_stale_local && cli.delete {
                *decision = Decision::Delete;
                *reason = Some("stale-local-metadata");
            } else {
                *decision = Decision::Stale;
                *reason = Some("stale-local-metadata");
            }
        }
        PersonalValidationOutcome::Invalid(problems)
            if has_metadata_kind(&problems, ThumbnailMetadataProblemKind::InvalidSyntax) =>
        {
            *decision = Decision::Delete;
            *reason = Some("invalid-metadata-syntax");
        }
        PersonalValidationOutcome::Invalid(problems)
            if has_metadata_kind(&problems, ThumbnailMetadataProblemKind::MissingRequired) =>
        {
            *decision = Decision::Delete;
            *reason = Some("missing-required-metadata");
        }
        PersonalValidationOutcome::Invalid(problems) if only_nonconforming(&problems) => {
            *decision = Decision::Skip;
            *reason = first_nonconforming_reason(&problems);
        }
        PersonalValidationOutcome::Invalid(_) => {
            *decision = Decision::Skip;
            *reason = Some("original-unverifiable");
        }
        _ => {
            *decision = Decision::Skip;
            *reason = Some("original-unverifiable");
        }
    }
}

fn has_metadata_kind(problems: &[CacheEntryProblem], kind: ThumbnailMetadataProblemKind) -> bool {
    problems.iter().any(|problem| {
        matches!(problem, CacheEntryProblem::Metadata(metadata) if metadata.kind() == kind)
    })
}

fn only_nonconforming(problems: &[CacheEntryProblem]) -> bool {
    !problems.is_empty()
        && problems.iter().all(|problem| {
            matches!(
                problem,
                CacheEntryProblem::NonconformingPngFormat
                    | CacheEntryProblem::DimensionsExceedNamespace
            )
        })
}

fn first_nonconforming_reason(problems: &[CacheEntryProblem]) -> Option<&'static str> {
    if problems.contains(&CacheEntryProblem::NonconformingPngFormat) {
        Some("nonconforming-format")
    } else if problems.contains(&CacheEntryProblem::DimensionsExceedNamespace) {
        Some("nonconforming-dimensions")
    } else {
        None
    }
}

fn record_nonfatal_error(summary: &mut Summary) {
    summary.nonfatal_error = true;
    summary.errors += 1;
}

fn is_not_found_error(error: &ThumbnailError) -> bool {
    matches!(
        error,
        ThumbnailError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
    )
}

fn evaluate_age_based(
    entry: &CacheEntryInspection,
    cli: &Cli,
    classification: UriClass,
    timestamp: Option<SystemTime>,
    decision: &mut Decision,
    reason: &mut Option<&'static str>,
    summary: &mut Summary,
) {
    if cli.age_basis == AgeBasisArg::AccessTime
        && entry.timestamps().access_time_preserved_during_inspection()
            != AccessTimePreservation::Preserved
    {
        *decision = Decision::Skip;
        *reason = Some("timestamp-preservation-unavailable");
        summary.timestamp_preservation_unavailable += 1;
        return;
    }
    let Some(timestamp) = timestamp else {
        *decision = Decision::Skip;
        *reason = Some("timestamp-unavailable");
        summary.timestamp_unavailable += 1;
        return;
    };
    let older_than_threshold = SystemTime::now()
        .duration_since(timestamp)
        .is_ok_and(|age| age >= cli.older_than);
    if older_than_threshold {
        *decision = Decision::Delete;
        *reason = Some(match classification {
            UriClass::Remote => "remote-older-than-threshold",
            UriClass::ArchiveOrVirtual => "virtual-older-than-threshold",
            UriClass::LocalRemovableOrPortal => "removable-older-than-threshold",
            UriClass::LocalStableFile | UriClass::Unknown => "older-than-threshold",
        });
    }
}

fn write_human(records: &[EntryRecord], summary: &Summary, age_basis: AgeBasisArg, verbose: bool) {
    for record in records {
        if record.decision == "keep" && !verbose {
            continue;
        }
        let action = if record
            .error
            .as_ref()
            .is_some_and(|error| error.kind == "delete-failed")
        {
            "delete-failed"
        } else if record.decision == "delete" && record.applied {
            "deleted"
        } else if record.decision == "delete" {
            "would-delete"
        } else {
            record.decision
        };
        println!(
            "{action} {} uri={} class={} decision={} applied={} reason={} basis={} error={}",
            record.thumbnail_path_display,
            record.uri.as_deref().unwrap_or(""),
            record.classification,
            record.decision,
            record.applied,
            record.reason.unwrap_or("none"),
            record.age_basis,
            record
                .error
                .as_ref()
                .map_or("none".to_owned(), |error| format!(
                    "{}:{}",
                    error.kind, error.message
                ))
        );
    }
    println!(
        "summary scanned={} kept={} would-delete={} deleted={} skipped={} errors={} basis={} timestamp-unavailable={} timestamp-unreliable={} timestamp-preservation-unavailable={}",
        summary.scanned,
        summary.kept,
        summary.would_delete,
        summary.deleted,
        summary.skipped,
        summary.errors,
        age_basis_name(age_basis),
        summary.timestamp_unavailable,
        summary.timestamp_unreliable,
        summary.timestamp_preservation_unavailable
    );
    if age_basis == AgeBasisArg::AccessTime
        && (summary.timestamp_unavailable > 0
            || summary.timestamp_unreliable > 0
            || summary.timestamp_preservation_unavailable > 0)
    {
        println!(
            "hint: mtime cleanup is more portable and more aggressive; for example: xdg-thumbnail-prune --older-than 30d --age-basis mtime"
        );
    }
}

fn write_jsonl(records: &[EntryRecord], summary: &Summary, age_basis: AgeBasisArg) {
    for record in records {
        println!(
            "{}",
            serde_json::to_string(record).expect("serialize entry record")
        );
    }
    let summary_record = SummaryRecord {
        schema_version: 0,
        event: "summary",
        scanned: summary.scanned,
        kept: summary.kept,
        would_delete: summary.would_delete,
        deleted: summary.deleted,
        skipped: summary.skipped,
        errors: summary.errors,
        age_basis: age_basis_name(age_basis),
        timestamp_unavailable: summary.timestamp_unavailable,
        timestamp_unreliable: summary.timestamp_unreliable,
        timestamp_preservation_unavailable: summary.timestamp_preservation_unavailable,
    };
    println!(
        "{}",
        serde_json::to_string(&summary_record).expect("serialize summary record")
    );
}

struct Classifier {
    removable_prefixes: Vec<PathBuf>,
    ignore_fhs_media: bool,
}

impl Classifier {
    fn new(cli: &Cli) -> Self {
        let mut removable_prefixes = cli.removable_prefix.clone();
        let uid = rustix::process::getuid().as_raw();
        removable_prefixes.extend([
            PathBuf::from(format!("/run/media/{uid}")),
            PathBuf::from(format!("/run/user/{uid}/doc")),
            PathBuf::from(format!("/run/user/{uid}/gvfs")),
            PathBuf::from(format!("/run/user/{uid}/kio-fuse")),
        ]);
        if !cli.ignore_fhs_media {
            removable_prefixes.push(PathBuf::from("/media"));
        }
        Self {
            removable_prefixes,
            ignore_fhs_media: cli.ignore_fhs_media,
        }
    }

    fn classify(&self, uri: &PersonalOriginalUri) -> UriClass {
        let text = uri.as_str();
        let scheme = text.split_once(':').map_or("", |(scheme, _)| scheme);
        match scheme {
            "file" => {
                let Some(path) = local_file_uri_to_path(text) else {
                    return UriClass::Unknown;
                };
                if self
                    .removable_prefixes
                    .iter()
                    .any(|prefix| path.starts_with(prefix))
                {
                    UriClass::LocalRemovableOrPortal
                } else {
                    let _ = self.ignore_fhs_media;
                    UriClass::LocalStableFile
                }
            }
            "http" | "https" | "ftp" | "sftp" | "smb" | "dav" => UriClass::Remote,
            "zip" | "tar" | "trash" | "recent" | "recentlyused" | "mtp" | "krarc" | "sevenz"
            | "rar" | "gdrive" | "timeline" | "tags" | "applications" | "desktop" | "programs"
            | "fonts" | "remote" | "network" | "bluetooth" | "camera" | "audiocd" | "obexftp"
            | "thumbnail" | "activities" | "filenamesearch" | "baloosearch" | "zeroconf" => {
                UriClass::ArchiveOrVirtual
            }
            _ => UriClass::Unknown,
        }
    }
}

fn personal_uri(entry: &CacheEntryInspection) -> Option<&PersonalOriginalUri> {
    match entry.original_uri() {
        Some(OriginalUriIdentity::Personal(uri)) => Some(uri),
        _ => None,
    }
}

fn successful_size(namespace: &CacheNamespace) -> Option<ThumbnailSize> {
    match namespace {
        CacheNamespace::Size(size) => Some(*size),
        CacheNamespace::Failure(_) => None,
        _ => None,
    }
}

fn is_failure_namespace(namespace: &CacheNamespace) -> bool {
    matches!(namespace, CacheNamespace::Failure(_))
}

fn selected_timestamp(entry: &CacheEntryInspection, basis: AgeBasisArg) -> Option<SystemTime> {
    match basis {
        AgeBasisArg::AccessTime => entry.timestamps().accessed_at(),
        AgeBasisArg::ModificationTime => entry.timestamps().modified_at(),
    }
}

fn age_basis_name(age_basis: AgeBasisArg) -> &'static str {
    match age_basis {
        AgeBasisArg::AccessTime => "atime",
        AgeBasisArg::ModificationTime => "mtime",
    }
}

fn access_preservation_name(value: AccessTimePreservation) -> &'static str {
    match value {
        AccessTimePreservation::Preserved => "preserved",
        AccessTimePreservation::NotPreserved => "not-preserved",
        AccessTimePreservation::NotNeeded => "not-needed",
        AccessTimePreservation::Unsupported => "unsupported",
        _ => "unknown",
    }
}

fn system_time_seconds(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

#[cfg(unix)]
fn path_bytes_b64(path: &Path) -> Option<String> {
    Some(base64::engine::general_purpose::STANDARD_NO_PAD.encode(path.as_os_str().as_bytes()))
}

#[cfg(not(unix))]
fn path_bytes_b64(_path: &Path) -> Option<String> {
    None
}

#[cfg(unix)]
fn local_file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file:")?;
    let path = if let Some(path) = rest.strip_prefix("///") {
        format!("/{path}")
    } else if let Some(path) = rest.strip_prefix("//localhost/") {
        format!("/{path}")
    } else {
        return None;
    };
    let bytes = percent_decode(path.as_bytes())?;
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(not(unix))]
fn local_file_uri_to_path(_uri: &str) -> Option<PathBuf> {
    None
}

fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            let high = hex_value(*input.get(i + 1)?)?;
            let low = hex_value(*input.get(i + 2)?)?;
            output.push(high << 4 | low);
            i += 3;
        } else {
            output.push(input[i]);
            i += 1;
        }
    }
    Some(output)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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
