// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Freedesktop thumbnail cache primitives.

use std::fmt;

use thiserror::Error;

/// Errors returned by thumbnail cache identity and filesystem operations.
#[derive(Debug, Error)]
pub enum ThumbnailError {
    /// The requested operation depends on Unix path byte semantics.
    #[error("operation is unsupported on this platform")]
    UnsupportedPlatform,
    /// A URI or filename input is not valid for the requested thumbnail context.
    #[error("invalid thumbnail URI identity: {0}")]
    InvalidUriIdentity(&'static str),
    /// A cache namespace is not valid for filesystem use.
    #[error("invalid cache namespace: {0}")]
    InvalidNamespace(&'static str),
    /// The cache root could not be resolved from XDG environment variables.
    #[error("cache root could not be resolved: {0}")]
    CacheRootUnavailable(&'static str),
    /// Filesystem I/O failed.
    #[error("{context}: {source}")]
    Io {
        /// Operation that failed.
        context: &'static str,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// PNG data could not be decoded or encoded.
    #[error("png error: {0}")]
    Png(String),
    /// Thumbnail metadata is invalid.
    #[error("invalid thumbnail metadata: {0}")]
    InvalidMetadata(&'static str),
    /// Rendered thumbnail payload is unsupported.
    #[error("unsupported rendered thumbnail: {0}")]
    UnsupportedRenderedThumbnail(&'static str),
}

impl ThumbnailError {
    fn invalid_uri(reason: &'static str) -> Self {
        Self::InvalidUriIdentity(reason)
    }
}

/// Result type used by this crate.
pub type Result<T, E = ThumbnailError> = std::result::Result<T, E>;

/// A canonical absolute URI identity for entries in the personal thumbnail cache.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PersonalThumbnailUri {
    value: String,
}

impl PersonalThumbnailUri {
    /// Constructs a canonical `file:///` URI from absolute Unix path bytes.
    ///
    /// This constructor performs byte-level percent-encoding and never expands
    /// shell syntax or resolves symlinks.
    #[cfg(unix)]
    pub fn from_absolute_path_bytes(path: &[u8]) -> Result<Self> {
        if !path.starts_with(b"/") {
            return Err(ThumbnailError::invalid_uri("local path must be absolute"));
        }
        if path.contains(&0) {
            return Err(ThumbnailError::invalid_uri(
                "local path must not contain NUL",
            ));
        }

        Ok(Self {
            value: format!("file://{}", encode_uri_path_bytes(path, true)),
        })
    }

    /// Constructs a canonical `file:///` URI from absolute Unix path bytes.
    #[cfg(not(unix))]
    pub fn from_absolute_path_bytes(_path: &[u8]) -> Result<Self> {
        Err(ThumbnailError::UnsupportedPlatform)
    }

    /// Accepts textual local `file:` URI input and normalizes `localhost`.
    pub fn from_local_file_uri(uri: &str) -> Result<Self> {
        validate_ascii_uri_identity(uri)?;
        let rest = uri
            .strip_prefix("file:")
            .ok_or_else(|| ThumbnailError::invalid_uri("local URI must use the file scheme"))?;

        let normalized = if let Some(path) = rest.strip_prefix("//localhost/") {
            format!("file:///{path}")
        } else if rest.starts_with("///") {
            uri.to_owned()
        } else if rest.starts_with("//") {
            return Err(ThumbnailError::invalid_uri(
                "file URI authority is not directly local",
            ));
        } else if rest.starts_with('/') {
            format!("file://{rest}")
        } else {
            return Err(ThumbnailError::invalid_uri(
                "file URI path must be absolute",
            ));
        };

        let path = normalized
            .strip_prefix("file://")
            .ok_or_else(|| ThumbnailError::invalid_uri("local URI must use the file scheme"))?;
        if !path.starts_with('/') {
            return Err(ThumbnailError::invalid_uri(
                "file URI path must be absolute",
            ));
        }
        validate_percent_escapes(path.as_bytes())?;

        Ok(Self { value: normalized })
    }

    /// Accepts a caller-selected absolute thumbnail URI identity and preserves it exactly.
    pub fn from_absolute_uri(uri: &str) -> Result<Self> {
        validate_ascii_uri_identity(uri)?;
        let scheme_end = uri
            .find(':')
            .ok_or_else(|| ThumbnailError::invalid_uri("URI must be absolute"))?;
        validate_scheme(&uri[..scheme_end])?;
        validate_percent_escapes(uri.as_bytes())?;

        Ok(Self {
            value: uri.to_owned(),
        })
    }

    /// Returns the canonical URI identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the Freedesktop MD5 filename stem for this URI identity.
    #[must_use]
    pub fn md5_stem(&self) -> String {
        md5_stem(self.value.as_bytes())
    }

    /// Returns the Freedesktop thumbnail filename for this URI identity.
    #[must_use]
    pub fn thumbnail_filename(&self) -> String {
        format!("{}.png", self.md5_stem())
    }
}

impl fmt::Display for PersonalThumbnailUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

/// A canonical `./`-prefixed URI identity for direct children in shared repositories.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SharedRelativeThumbnailUri {
    value: String,
}

impl SharedRelativeThumbnailUri {
    /// Constructs a shared URI from one raw direct child filename.
    pub fn from_raw_child_name(name: &[u8]) -> Result<Self> {
        validate_raw_shared_child_name(name)?;

        Ok(Self {
            value: format!("./{}", encode_uri_path_bytes(name, false)),
        })
    }

    /// Parses textual `./` shared URI input without allowing encoded path separators.
    pub fn parse(uri: &str) -> Result<Self> {
        validate_ascii_uri_identity(uri)?;
        let encoded = uri
            .strip_prefix("./")
            .ok_or_else(|| ThumbnailError::invalid_uri("shared URI must start with ./"))?;
        if encoded.is_empty() {
            return Err(ThumbnailError::invalid_uri(
                "shared URI child name must not be empty",
            ));
        }
        if encoded.contains('/') {
            return Err(ThumbnailError::invalid_uri(
                "shared URI must name one direct child",
            ));
        }
        validate_percent_escapes(encoded.as_bytes())?;
        let decoded = percent_decode_bytes(encoded.as_bytes())?;
        validate_raw_shared_child_name(&decoded)?;

        Ok(Self {
            value: uri.to_owned(),
        })
    }

    /// Returns the canonical shared relative URI identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the Freedesktop MD5 filename stem for this URI identity.
    #[must_use]
    pub fn md5_stem(&self) -> String {
        md5_stem(self.value.as_bytes())
    }

    /// Returns the Freedesktop thumbnail filename for this URI identity.
    #[must_use]
    pub fn thumbnail_filename(&self) -> String {
        format!("{}.png", self.md5_stem())
    }
}

impl fmt::Display for SharedRelativeThumbnailUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

fn md5_stem(input: &[u8]) -> String {
    format!("{:x}", md5::compute(input))
}

fn validate_raw_shared_child_name(name: &[u8]) -> Result<()> {
    if name.is_empty() {
        return Err(ThumbnailError::invalid_uri(
            "shared child name must not be empty",
        ));
    }
    if name == b"." || name == b".." {
        return Err(ThumbnailError::invalid_uri(
            "shared child name must not be . or ..",
        ));
    }
    if name.contains(&b'/') || name.contains(&0) {
        return Err(ThumbnailError::invalid_uri(
            "shared child name must be one path segment",
        ));
    }
    Ok(())
}

fn validate_scheme(scheme: &str) -> Result<()> {
    let mut bytes = scheme.bytes();
    let Some(first) = bytes.next() else {
        return Err(ThumbnailError::invalid_uri("URI scheme must not be empty"));
    };
    if !first.is_ascii_alphabetic() {
        return Err(ThumbnailError::invalid_uri(
            "URI scheme must start with an ASCII letter",
        ));
    }
    if !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')) {
        return Err(ThumbnailError::invalid_uri(
            "URI scheme contains an invalid character",
        ));
    }
    Ok(())
}

fn validate_ascii_uri_identity(uri: &str) -> Result<()> {
    if uri.is_empty() {
        return Err(ThumbnailError::invalid_uri("URI must not be empty"));
    }
    if !uri.is_ascii() {
        return Err(ThumbnailError::invalid_uri(
            "URI identity must be ASCII and percent-encoded",
        ));
    }
    if uri.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ThumbnailError::invalid_uri(
            "URI identity must not contain control characters",
        ));
    }
    Ok(())
}

fn validate_percent_escapes(input: &[u8]) -> Result<()> {
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            if i + 2 >= input.len()
                || !input[i + 1].is_ascii_hexdigit()
                || !input[i + 2].is_ascii_hexdigit()
            {
                return Err(ThumbnailError::invalid_uri(
                    "URI contains an invalid percent escape",
                ));
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    Ok(())
}

fn percent_decode_bytes(input: &[u8]) -> Result<Vec<u8>> {
    validate_percent_escapes(input)?;
    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            let high = hex_value(input[i + 1]).ok_or_else(|| {
                ThumbnailError::invalid_uri("URI contains an invalid percent escape")
            })?;
            let low = hex_value(input[i + 2]).ok_or_else(|| {
                ThumbnailError::invalid_uri("URI contains an invalid percent escape")
            })?;
            output.push(high << 4 | low);
            i += 3;
        } else {
            output.push(input[i]);
            i += 1;
        }
    }
    Ok(output)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn encode_uri_path_bytes(bytes: &[u8], allow_slash: bool) -> String {
    let mut encoded = String::with_capacity(bytes.len());
    for &byte in bytes {
        if is_safe_path_byte(byte, allow_slash) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn is_safe_path_byte(byte: u8, allow_slash: bool) -> bool {
    byte.is_ascii_alphanumeric()
        || (allow_slash && byte == b'/')
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

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
