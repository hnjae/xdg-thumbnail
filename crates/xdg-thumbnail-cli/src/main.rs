// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

use xdg_thumbnail::ThumbnailSize;

#[allow(dead_code)]
mod policy {
    /// A cleanup-oriented URI classification owned by the CLI policy layer.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum UriClass {
        /// A local file whose path can be checked directly.
        LocalStableFile,
        /// A local-looking file whose backing storage may be temporarily unavailable.
        LocalRemovableOrPortal,
        /// A network or internet-related URI.
        Remote,
        /// A virtual, archive, or desktop-environment-specific URI.
        ArchiveOrVirtual,
        /// A URI that cannot be classified confidently.
        Unknown,
    }

    /// A deletion reason returned by cleanup policy evaluation.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum DeleteReason {
        /// The original stable local file is missing.
        OriginalMissing,
        /// A remote thumbnail is older than the configured threshold.
        RemoteOlderThanThreshold,
        /// A virtual or archive thumbnail is older than the configured threshold.
        VirtualOlderThanThreshold,
        /// A removable-media-like thumbnail is older than the configured threshold.
        RemovableOlderThanThreshold,
        /// The thumbnail file or required metadata is malformed.
        Malformed,
    }

    /// A reason for skipping a cache entry.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum SkipReason {
        /// The entry is outside the current scan policy.
        OutOfScope,
        /// The entry could not be inspected because of filesystem permissions or I/O errors.
        Unreadable,
    }

    /// A cleanup decision produced from cache state and caller policy.
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub enum CleanupDecision {
        /// Keep the thumbnail as-is.
        Keep,
        /// Delete the thumbnail for the given reason.
        Delete(DeleteReason),
        /// The thumbnail is stale and should be recreated by an application.
        Recreate,
        /// Skip the thumbnail for the given reason.
        Skip(SkipReason),
    }
}

fn main() {
    let sizes = ThumbnailSize::all()
        .map(ThumbnailSize::directory_name)
        .join(", ");

    println!("xdg-thumbnail {} ({sizes})", env!("CARGO_PKG_VERSION"));
}
