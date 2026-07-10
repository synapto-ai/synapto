use serde::{Deserialize, Serialize};
use std::fmt;

/// A wrapper that redacts its contents in Debug logs and Serialization (UI/API),
/// requiring explicit method calls to access the underlying value.
#[derive(Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    /// Creates a new Secret.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Explicitly exposes the secret by reference.
    #[inline]
    pub fn expose_secret(&self) -> &T {
        &self.0
    }

    /// Consumes the wrapper and returns the raw secret.
    #[inline]
    pub fn expose_owned_secret(self) -> T {
        self.0
    }

    #[inline]
    pub fn into_secret<U>(self) -> Secret<U>
    where
        T: Into<U>,
    {
        Secret(self.0.into())
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl<T> Serialize for Secret<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Always serializes as a generic string, regardless of inner type T
        serializer.serialize_str("[REDACTED]")
    }
}
