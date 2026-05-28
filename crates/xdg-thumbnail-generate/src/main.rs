// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use xdg_thumbnail::ThumbnailSize;

#[allow(dead_code)]
mod policy {
    /// Thumbnailer sandbox policy selected by the caller.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum SandboxMode {
        /// Require thumbnailer execution inside the configured sandbox backend.
        Required,
        /// Execute the selected thumbnailer without sandbox isolation.
        Off,
    }

    /// A high-level sandbox eligibility result for a selected thumbnailer.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum SandboxEligibility {
        /// The selected thumbnailer can run under the requested sandbox policy.
        Eligible,
        /// The sandbox backend is unavailable on this host.
        BackendUnavailable,
        /// The sandbox cannot expose the selected executable or runtime inputs safely.
        RuntimeExposureUnavailable,
        /// The sandbox cannot provide the required network isolation.
        NetworkIsolationUnavailable,
    }

    /// A reason for skipping a requested input-size pair without running a thumbnailer.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum SkipReason {
        /// The input path is outside the supported local filesystem input scope.
        UnsupportedInput,
        /// The input could not be opened for reading.
        InputUnreadable,
        /// The input modification time could not be obtained.
        OriginalMetadataUnavailable,
        /// The MIME type could not be determined.
        UnknownMimeType,
        /// No installed thumbnailer matches the detected MIME type.
        NoMatchingThumbnailer,
    }

    /// A failure reason for a selected generation attempt.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum FailureReason {
        /// The selected thumbnailer entry is invalid.
        InvalidThumbnailerEntry,
        /// The selected thumbnailer cannot run under the requested sandbox policy.
        SandboxIneligible,
        /// The thumbnailer process exited unsuccessfully.
        ThumbnailerFailed,
        /// The thumbnailer process exceeded the configured timeout.
        Timeout,
        /// The temporary thumbnail output is missing or invalid.
        InvalidOutput,
        /// The generated thumbnail could not be installed atomically.
        CacheInstallFailed,
    }

    /// A generation decision produced for one requested input-size pair.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum GenerationDecision {
        /// Keep the existing valid thumbnail as-is.
        Keep,
        /// Run the selected thumbnailer and install its output if validation succeeds.
        Generate,
        /// Skip the pair before selecting or running a thumbnailer.
        Skip(SkipReason),
        /// Report a failed selected generation attempt.
        Fail(FailureReason),
    }
}

fn main() {
    let sizes = ThumbnailSize::all()
        .map(ThumbnailSize::directory_name)
        .join(", ");

    println!(
        "{} {} ({sizes})",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
}
