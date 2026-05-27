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
    /// The original cannot be verified through the available validation context.
    UnverifiableOriginal,
    /// The thumbnail file or metadata is malformed.
    Malformed,
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
