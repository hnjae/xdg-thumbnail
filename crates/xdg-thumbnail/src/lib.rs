// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Freedesktop thumbnail cache primitives.

/// A standard Freedesktop thumbnail size directory.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ThumbnailSize {
    /// 128px thumbnail cache directory.
    Normal,
    /// 256px thumbnail cache directory.
    Large,
    /// 512px thumbnail cache directory.
    XLarge,
    /// 1024px thumbnail cache directory.
    XxLarge,
}

impl ThumbnailSize {
    /// Returns the standard cache directory name for this size.
    #[must_use]
    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
            Self::XLarge => "x-large",
            Self::XxLarge => "xx-large",
        }
    }

    /// Returns all standard thumbnail sizes in cache scan order.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Normal, Self::Large, Self::XLarge, Self::XxLarge]
    }
}

/// A cleanup-oriented URI classification.
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

/// Validation state for a thumbnail cache entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CacheEntryState {
    /// The thumbnail metadata still matches the original.
    Valid,
    /// The original local file no longer exists.
    OriginalMissing,
    /// The original exists but metadata no longer matches.
    Outdated,
    /// The original could not be read well enough to validate the entry.
    UnreadableOriginal,
    /// The original is remote, virtual, or otherwise not directly checkable.
    RemoteOrUnknown,
    /// The thumbnail file or metadata is malformed.
    Malformed,
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
}

/// A reason for skipping a cache entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SkipReason {
    /// The thumbnail file or required metadata cannot be parsed.
    Malformed,
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

#[cfg(test)]
mod tests {
    use super::ThumbnailSize;

    #[test]
    fn thumbnail_size_directory_names_match_standard() {
        assert_eq!(ThumbnailSize::Normal.directory_name(), "normal");
        assert_eq!(ThumbnailSize::Large.directory_name(), "large");
        assert_eq!(ThumbnailSize::XLarge.directory_name(), "x-large");
        assert_eq!(ThumbnailSize::XxLarge.directory_name(), "xx-large");
    }

    #[test]
    fn all_thumbnail_sizes_are_in_scan_order() {
        let names = ThumbnailSize::all().map(ThumbnailSize::directory_name);

        assert_eq!(names, ["normal", "large", "x-large", "xx-large"]);
    }
}
