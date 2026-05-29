// SPDX-FileCopyrightText: 2026 KIM Hyunjae
// SPDX-License-Identifier: MPL-2.0

use std::fmt;

use crate::{Result, ThumbnailError};

/// A canonical absolute URI identity for entries in the personal thumbnail cache.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PersonalOriginalUri {
    value: String,
}

impl PersonalOriginalUri {
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

    /// Accepts textual local `file:` URI input and normalizes local file URI casing.
    pub fn from_local_file_uri(uri: &str) -> Result<Self> {
        validate_ascii_uri_identity(uri)?;
        let scheme_end = uri
            .find(':')
            .ok_or_else(|| ThumbnailError::invalid_uri("local URI must use the file scheme"))?;
        let scheme = &uri[..scheme_end];
        validate_scheme(scheme)?;
        if !scheme.eq_ignore_ascii_case("file") {
            return Err(ThumbnailError::invalid_uri(
                "local URI must use the file scheme",
            ));
        }
        let rest = &uri[scheme_end + 1..];

        let path = if let Some(rest) = rest.strip_prefix("//") {
            let (authority, path) = rest
                .split_once('/')
                .ok_or_else(|| ThumbnailError::invalid_uri("file URI path must be absolute"))?;
            if !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) {
                return Err(ThumbnailError::invalid_uri(
                    "file URI authority is not directly local",
                ));
            }
            format!("/{path}")
        } else if rest.starts_with('/') {
            rest.to_owned()
        } else {
            return Err(ThumbnailError::invalid_uri(
                "file URI path must be absolute",
            ));
        };
        if !path.starts_with('/') {
            return Err(ThumbnailError::invalid_uri(
                "file URI path must be absolute",
            ));
        }
        validate_uri_path_text(path.as_bytes(), true)?;
        let path_bytes = percent_decode_bytes(path.as_bytes())?;
        Self::from_absolute_path_bytes(&path_bytes)
    }

    /// Accepts a caller-selected non-local absolute thumbnail URI identity and preserves it exactly.
    pub fn from_caller_selected_absolute_uri(uri: &str) -> Result<Self> {
        let scheme = validate_absolute_uri_identity(uri)?;
        if scheme.eq_ignore_ascii_case("file") {
            return Err(ThumbnailError::invalid_uri(
                "caller-selected URI identity must not use the file scheme",
            ));
        }

        Ok(Self {
            value: uri.to_owned(),
        })
    }

    pub(crate) fn from_validated_absolute_uri(uri: &str) -> Result<Self> {
        validate_absolute_uri_identity(uri)?;
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

impl fmt::Display for PersonalOriginalUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

/// A canonical `./`-prefixed URI identity for direct children in shared repositories.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SharedRelativeOriginalUri {
    value: String,
}

impl SharedRelativeOriginalUri {
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
        validate_uri_path_text(encoded.as_bytes(), false)?;
        let decoded = percent_decode_bytes(encoded.as_bytes())?;
        Self::from_raw_child_name(&decoded)
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

impl fmt::Display for SharedRelativeOriginalUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

fn md5_stem(input: &[u8]) -> String {
    format!("{:x}", md5::compute(input))
}

pub(crate) fn validate_absolute_uri_identity(uri: &str) -> Result<&str> {
    validate_ascii_uri_identity(uri)?;
    let scheme_end = uri
        .find(':')
        .ok_or_else(|| ThumbnailError::invalid_uri("URI must be absolute"))?;
    let scheme = &uri[..scheme_end];
    validate_scheme(scheme)?;
    validate_percent_escapes(uri.as_bytes())?;
    Ok(scheme)
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
    if uri
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return Err(ThumbnailError::invalid_uri(
            "URI identity must not contain control characters or spaces",
        ));
    }
    Ok(())
}

fn validate_uri_path_text(input: &[u8], allow_slash: bool) -> Result<()> {
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
        } else if is_safe_path_byte(input[i], allow_slash) {
            i += 1;
        } else {
            return Err(ThumbnailError::invalid_uri(
                "URI path contains an unescaped byte that must be percent-encoded",
            ));
        }
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
