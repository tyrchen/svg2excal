//! Explicit bounded-resource provider contract.

use std::fmt::Debug;

use thiserror::Error;

/// Policy for caller-provided relative files.
#[derive(Debug, Clone)]
pub struct RelativeFilePolicy {
    max_path_bytes: usize,
}

impl RelativeFilePolicy {
    /// Creates a relative-file policy with a bounded path length.
    ///
    /// # Errors
    ///
    /// Returns an error when the bound is zero or exceeds 4096 bytes.
    pub fn new(max_path_bytes: usize) -> Result<Self, ResourceError> {
        if !(1..=4096).contains(&max_path_bytes) {
            return Err(ResourceError::InvalidPolicy);
        }
        Ok(Self { max_path_bytes })
    }

    /// Maximum lexical path bytes.
    #[must_use]
    pub const fn max_path_bytes(&self) -> usize {
        self.max_path_bytes
    }
}

impl Default for RelativeFilePolicy {
    fn default() -> Self {
        Self {
            max_path_bytes: 1024,
        }
    }
}

/// Core-enforced resource policy. V1 intentionally has no network variant.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ProvidedResourcePolicy {
    /// Admit validated relative path requests through the provider.
    RelativeFiles(RelativeFilePolicy),
}

/// Validated request passed to a resource provider.
#[derive(Clone)]
pub struct ResourceRequest {
    relative_path: String,
}

impl ResourceRequest {
    pub(crate) fn parse(path: &str, policy: &RelativeFilePolicy) -> Result<Self, ResourceError> {
        let valid_charset = path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'));
        let valid_components = !path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..");
        if path.is_empty()
            || path.len() > policy.max_path_bytes
            || path.starts_with('/')
            || path.contains('\0')
            || path.contains('\\')
            || path.contains(':')
            || !valid_charset
            || !valid_components
        {
            return Err(ResourceError::InvalidPath);
        }
        Ok(Self {
            relative_path: path.to_owned(),
        })
    }

    /// Validated relative path.
    #[must_use]
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }
}

impl Debug for ResourceRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResourceRequest")
            .field("relative_path", &"<redacted>")
            .field("bytes", &self.relative_path.len())
            .finish()
    }
}

/// Bytes and declared MIME returned by a provider.
#[derive(Clone)]
pub struct ProvidedResource {
    mime_type: String,
    bytes: Vec<u8>,
}

impl ProvidedResource {
    /// Creates a resource value. Core revalidates MIME, magic, and size after loading.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty/oversized MIME label or empty bytes.
    pub fn new(mime_type: &str, bytes: Vec<u8>) -> Result<Self, ResourceError> {
        let valid_mime = (1..=64).contains(&mime_type.len())
            && mime_type.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'/' | b'+' | b'-' | b'.')
            });
        if !valid_mime {
            return Err(ResourceError::InvalidMime);
        }
        if bytes.is_empty() {
            return Err(ResourceError::EmptyResource);
        }
        Ok(Self {
            mime_type: mime_type.to_owned(),
            bytes,
        })
    }

    /// Declared lower-case MIME type.
    #[must_use]
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// Provider bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Debug for ProvidedResource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProvidedResource")
            .field("mime_type", &self.mime_type)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Provider failure. Messages are fixed and contain no source path or bytes.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ResourceError {
    /// Policy bounds are invalid.
    #[error("invalid resource policy")]
    InvalidPolicy,
    /// Relative path validation failed.
    #[error("invalid relative resource path")]
    InvalidPath,
    /// MIME label validation failed.
    #[error("invalid resource MIME type")]
    InvalidMime,
    /// Provider returned an empty resource.
    #[error("provider returned an empty resource")]
    EmptyResource,
    /// Provider could not locate the validated resource.
    #[error("resource not found")]
    NotFound,
    /// Provider I/O failed.
    #[error("resource provider I/O failed")]
    Io,
}

/// Synchronous object-safe bounded resource provider.
pub trait ResourceProvider: Debug + Send + Sync {
    /// Loads one already-validated request.
    ///
    /// # Errors
    ///
    /// Returns a bounded [`ResourceError`]. Core revalidates successful bytes.
    fn load(&self, request: &ResourceRequest) -> Result<ProvidedResource, ResourceError>;
}

/// Couples an allow policy and provider so callbacks cannot widen authority.
#[derive(Debug)]
pub struct ResourceContext<'a> {
    pub(crate) policy: ProvidedResourcePolicy,
    pub(crate) provider: &'a dyn ResourceProvider,
}

impl<'a> ResourceContext<'a> {
    /// Creates an explicit resource context.
    #[must_use]
    pub const fn new(policy: ProvidedResourcePolicy, provider: &'a dyn ResourceProvider) -> Self {
        Self { policy, provider }
    }

    /// Core-enforced allow policy.
    #[must_use]
    pub const fn policy(&self) -> &ProvidedResourcePolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::{RelativeFilePolicy, ResourceRequest};

    #[test]
    fn test_should_reject_parent_traversal() {
        let result = ResourceRequest::parse("../secret.png", &RelativeFilePolicy::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_should_redact_request_debug() {
        let result = ResourceRequest::parse("images/secret.png", &RelativeFilePolicy::default());
        assert!(matches!(result, Ok(request) if !format!("{request:?}").contains("secret")));
    }
}
