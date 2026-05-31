// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::fmt;
use std::path::{Path, PathBuf};

use crate::{Result, ThumbnailError};

static THUMBNAIL_SIZES: [ThumbnailSize; 4] = [
    ThumbnailSize::Normal,
    ThumbnailSize::Large,
    ThumbnailSize::XLarge,
    ThumbnailSize::XxLarge,
];

/// A successful-thumbnail size namespace or a program failure namespace.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CacheNamespace {
    /// A successful thumbnail size directory.
    Size(ThumbnailSize),
    /// A failure-entry namespace under `fail/`.
    Failure(FailureNamespace),
}

impl CacheNamespace {
    pub(crate) fn join_under(&self, root: &Path, filename: &str) -> PathBuf {
        match self {
            Self::Size(size) => root.join(size.directory_name()).join(filename),
            Self::Failure(namespace) => root.join("fail").join(namespace.as_str()).join(filename),
        }
    }

    /// Returns the relative cache directory for this namespace.
    #[must_use]
    pub fn relative_directory(&self) -> String {
        match self {
            Self::Size(size) => size.directory_name().to_owned(),
            Self::Failure(namespace) => format!("fail/{}", namespace.as_str()),
        }
    }
}

impl fmt::Display for CacheNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.relative_directory())
    }
}

/// A validated direct directory name for failure entries.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FailureNamespace {
    value: String,
}

impl FailureNamespace {
    /// Creates a failure namespace from an ASCII direct directory name.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is empty, is `.` or `..`, or contains bytes outside ASCII
    /// letters, digits, `.`, `_`, `+`, and `-`.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() || value == "." || value == ".." {
            return Err(ThumbnailError::InvalidNamespace(
                "failure namespace must be a non-empty direct name",
            ));
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
        {
            return Err(ThumbnailError::InvalidNamespace(
                "failure namespace contains an invalid character",
            ));
        }
        Ok(Self { value })
    }

    /// Returns the namespace directory name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for FailureNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

/// A standard Freedesktop thumbnail size directory.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
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

    /// Returns the maximum width and height for this namespace in pixels.
    #[must_use]
    pub const fn max_dimension(self) -> u32 {
        match self {
            Self::Normal => 128,
            Self::Large => 256,
            Self::XLarge => 512,
            Self::XxLarge => 1024,
        }
    }

    /// Returns all standard thumbnail sizes in cache scan order.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &THUMBNAIL_SIZES
    }
}

#[cfg(test)]
mod tests {
    use super::{CacheNamespace, FailureNamespace, ThumbnailSize};

    #[test]
    fn thumbnail_size_directory_names_match_standard() {
        assert_eq!(ThumbnailSize::Normal.directory_name(), "normal");
        assert_eq!(ThumbnailSize::Large.directory_name(), "large");
        assert_eq!(ThumbnailSize::XLarge.directory_name(), "x-large");
        assert_eq!(ThumbnailSize::XxLarge.directory_name(), "xx-large");
    }

    #[test]
    fn all_thumbnail_sizes_are_in_scan_order() {
        let names = ThumbnailSize::all()
            .iter()
            .copied()
            .map(ThumbnailSize::directory_name)
            .collect::<Vec<_>>();

        assert_eq!(names, ["normal", "large", "x-large", "xx-large"]);
    }

    #[test]
    fn cache_namespace_strings_are_relative_directories() {
        let size = CacheNamespace::Size(ThumbnailSize::Normal);
        let failure = CacheNamespace::Failure(FailureNamespace::new("app-1").unwrap());

        assert_eq!(size.relative_directory(), "normal");
        assert_eq!(size.to_string(), "normal");
        assert_eq!(failure.relative_directory(), "fail/app-1");
        assert_eq!(failure.to_string(), "fail/app-1");
    }
}
