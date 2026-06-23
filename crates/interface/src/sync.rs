//! # Transparent Sync Proxy
//!
//! This module provides a drop-in replacement for `tokio::sync` primitives with built-in
//! telemetry instrumentation for Rerun pulses.
//!
//! ## Architecture
//!
//! - **Release Builds**: All types and functions are simple re-exports of `tokio::sync`.
//!   There is **absolute zero runtime cost**.
//! - **Debug Builds**: `mpsc` and `broadcast` channels are shadowed by instrumented wrappers.
//!   Every successful `send` or `try_send` emits a `tracing::trace!` pulse that is
//!   automatically captured by the core telemetry system.
//!
//! ## Automatic Naming
//!
//! Metrics are automatically named using the following format:
//! `semantic_name:channel_type:message_type@module_path`
//!
//! - **semantic_name**: Name provided via mandatory `register_channel_name!`.
//! - **channel_type**: `mpsc`, `unbounded`, or `broadcast`.
//! - **message_type**: Inferred from the generic type `T`.
//! - **module_path**: Inferred from the caller's location using `#[track_caller]`.

#[cfg(not(debug_assertions))]
pub use tokio::sync::*;

#[cfg(debug_assertions)]
pub mod mpsc {
    //! Instrumented `mpsc` channels.
    use std::ops::Deref;
    use tokio::sync::mpsc::error::{SendError, TrySendError};
    pub use tokio::sync::mpsc::{Receiver, UnboundedReceiver, error};

    /// Creates a bounded mpsc channel for communicating between asynchronous tasks.
    ///
    /// The returned `Sender` is instrumented with automatic telemetry pulses.
    /// The message type `T` must be registered via `register_channel_name!`.
    #[track_caller]
    pub fn channel<T: crate::sync::TypeChannelName>(buffer: usize) -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = tokio::sync::mpsc::channel(buffer);
        (Sender(tx), rx)
    }

    #[track_caller]
    pub fn unbounded_channel<T: crate::sync::TypeChannelName>()
    -> (UnboundedSender<T>, UnboundedReceiver<T>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (UnboundedSender(tx), rx)
    }

    /// An instrumented wrapper around `tokio::sync::mpsc::Sender`.
    #[derive(Debug)]
    #[repr(transparent)]
    pub struct Sender<T: crate::sync::TypeChannelName>(tokio::sync::mpsc::Sender<T>);

    impl<T: crate::sync::TypeChannelName> Clone for Sender<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<T: crate::sync::TypeChannelName> Sender<T> {
        /// Sends a value, waiting until there is capacity.
        ///
        /// Automatically emits a telemetry pulse upon successful send.
        #[track_caller]
        pub async fn send(&self, value: T) -> Result<(), SendError<T>> {
            let res = self.0.send(value).await;
            if res.is_ok() {
                super::detail::trace_pulse::<T>("mpsc", std::panic::Location::caller());
            }
            res
        }

        /// Sends a value, blocking the current thread until there is capacity.
        ///
        /// Automatically emits a telemetry pulse upon successful send.
        #[track_caller]
        pub fn blocking_send(&self, value: T) -> Result<(), SendError<T>> {
            let res = self.0.blocking_send(value);
            if res.is_ok() {
                super::detail::trace_pulse::<T>("mpsc", std::panic::Location::caller());
            }
            res
        }

        /// Attempts to send a value without waiting for capacity.
        ///
        /// Automatically emits a telemetry pulse upon successful send.
        #[track_caller]
        pub fn try_send(&self, message: T) -> Result<(), TrySendError<T>> {
            let res = self.0.try_send(message);
            if res.is_ok() {
                super::detail::trace_pulse::<T>("mpsc", std::panic::Location::caller());
            }
            res
        }
    }

    impl<T: crate::sync::TypeChannelName> Deref for Sender<T> {
        type Target = tokio::sync::mpsc::Sender<T>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<T: crate::sync::TypeChannelName> From<tokio::sync::mpsc::Sender<T>> for Sender<T> {
        fn from(inner: tokio::sync::mpsc::Sender<T>) -> Self {
            Self(inner)
        }
    }

    impl<T: crate::sync::TypeChannelName> From<Sender<T>> for tokio::sync::mpsc::Sender<T> {
        fn from(proxy: Sender<T>) -> Self {
            proxy.0
        }
    }

    /// An instrumented wrapper around `tokio::sync::mpsc::UnboundedSender`.
    #[derive(Debug)]
    #[repr(transparent)]
    pub struct UnboundedSender<T: crate::sync::TypeChannelName>(
        tokio::sync::mpsc::UnboundedSender<T>,
    );

    impl<T: crate::sync::TypeChannelName> Clone for UnboundedSender<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<T: crate::sync::TypeChannelName> UnboundedSender<T> {
        /// Sends a message to the corresponding `UnboundedReceiver`.
        ///
        /// Automatically emits a telemetry pulse upon successful send.
        #[track_caller]
        pub fn send(&self, message: T) -> Result<(), error::SendError<T>> {
            let res = self.0.send(message);
            if res.is_ok() {
                super::detail::trace_pulse::<T>("unbounded", std::panic::Location::caller());
            }
            res
        }
    }

    impl<T: crate::sync::TypeChannelName> Deref for UnboundedSender<T> {
        type Target = tokio::sync::mpsc::UnboundedSender<T>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<T: crate::sync::TypeChannelName> From<tokio::sync::mpsc::UnboundedSender<T>>
        for UnboundedSender<T>
    {
        fn from(inner: tokio::sync::mpsc::UnboundedSender<T>) -> Self {
            Self(inner)
        }
    }

    impl<T: crate::sync::TypeChannelName> From<UnboundedSender<T>>
        for tokio::sync::mpsc::UnboundedSender<T>
    {
        fn from(proxy: UnboundedSender<T>) -> Self {
            proxy.0
        }
    }
}

#[cfg(debug_assertions)]
pub mod broadcast {
    //! Instrumented `broadcast` channels.
    use std::ops::Deref;
    use tokio::sync::broadcast::error::SendError;
    pub use tokio::sync::broadcast::{Receiver, error};

    /// Creates a multi-producer, multi-consumer broadcast channel.
    ///
    /// The returned `Sender` is instrumented with automatic telemetry pulses.
    /// The message type `T` must be registered via `register_channel_name!`.
    #[track_caller]
    pub fn channel<T: Clone + crate::sync::TypeChannelName>(
        capacity: usize,
    ) -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = tokio::sync::broadcast::channel(capacity);
        (Sender(tx), rx)
    }

    /// An instrumented wrapper around `tokio::sync::broadcast::Sender`.
    #[derive(Debug)]
    #[repr(transparent)]
    pub struct Sender<T: Clone + crate::sync::TypeChannelName>(tokio::sync::broadcast::Sender<T>);

    impl<T: Clone + crate::sync::TypeChannelName> Clone for Sender<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<T: Clone + crate::sync::TypeChannelName> Sender<T> {
        /// Sends a message to all active receivers.
        ///
        /// Automatically emits a telemetry pulse upon successful send.
        #[track_caller]
        pub fn send(&self, value: T) -> Result<usize, SendError<T>> {
            let res = self.0.send(value);
            if res.is_ok() {
                super::detail::trace_pulse::<T>("broadcast", std::panic::Location::caller());
            }
            res
        }

        /// Subscribes to this broadcast channel.
        pub fn subscribe(&self) -> Receiver<T> {
            self.0.subscribe()
        }
    }

    impl<T: Clone + crate::sync::TypeChannelName> Deref for Sender<T> {
        type Target = tokio::sync::broadcast::Sender<T>;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<T: Clone + crate::sync::TypeChannelName> From<tokio::sync::broadcast::Sender<T>>
        for Sender<T>
    {
        fn from(inner: tokio::sync::broadcast::Sender<T>) -> Self {
            Self(inner)
        }
    }

    impl<T: Clone + crate::sync::TypeChannelName> From<Sender<T>>
        for tokio::sync::broadcast::Sender<T>
    {
        fn from(proxy: Sender<T>) -> Self {
            proxy.0
        }
    }
}

#[cfg(debug_assertions)]
pub use tokio::sync::{
    Barrier, Mutex, Notify, OnceCell, OwnedRwLockReadGuard, OwnedRwLockWriteGuard,
    OwnedSemaphorePermit, RwLock, RwLockReadGuard, RwLockWriteGuard, Semaphore, SemaphorePermit,
    TryLockError, futures, oneshot, watch,
};

/// Trait used to provide a semantic name for a type in telemetry pulses.
///
/// This is used by the `register_channel_name!` macro to associate a friendly
/// string with a specific message type.
pub trait TypeChannelName {
    /// The semantic name of the channel/message type.
    const CHANNEL_NAME: &'static str;
}

/// Registers a semantic name for a message type to be used in telemetry pulses.
///
/// ## Example
///
/// ```rust
/// use synapto_interface::register_channel_name;
/// pub struct MyMessage;
/// register_channel_name!(MyMessage, "my_custom_channel");
/// ```
///
/// This will result in Rerun metrics starting with `my_custom_channel:`.
#[macro_export]
macro_rules! register_channel_name {
    ($type_name:ty, $channel_name:expr) => {
        #[cfg(debug_assertions)]
        impl $crate::sync::TypeChannelName for $type_name {
            const CHANNEL_NAME: &'static str = $channel_name;
        }
    };
}

#[cfg(debug_assertions)]
mod detail {
    use std::panic::Location;

    #[inline(always)]
    pub fn trace_pulse<T: crate::sync::TypeChannelName>(
        channel_type: &'static str,
        location: &Location<'_>,
    ) {
        let type_name = std::any::type_name::<T>();
        let short_type = type_name
            .rsplit_once("::")
            .map(|(_, s)| s)
            .unwrap_or(type_name);

        let file = location.file();
        let filepath = if let Some((_, post_src)) = file.rsplit_once("src/") {
            post_src
                .strip_suffix(".rs")
                .unwrap_or(post_src)
                .strip_suffix("/mod")
                .unwrap_or(post_src)
                .replace(['/', '\\'], "::")
        } else {
            file.rsplit_once('/')
                .or_else(|| file.rsplit_once('\\'))
                .map(|(_, s)| s)
                .unwrap_or(file)
                .to_string()
        };

        let metric_name = format!(
            "{}:{}:{}@{}",
            T::CHANNEL_NAME,
            channel_type,
            short_type,
            filepath
        );
        tracing::trace!(target: "telemetry", metric = %metric_name, value = 1.0);
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    struct SemanticType;
    register_channel_name!(SemanticType, "semantic_channel");

    register_channel_name!(i32, "test_i32");

    #[tokio::test]
    async fn test_mpsc_proxy() {
        let (tx, mut rx) = mpsc::channel::<i32>(1);
        tx.send(42).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), 42);

        tx.try_send(43).unwrap();
        assert_eq!(rx.recv().await.unwrap(), 43);

        // Test Deref
        assert_eq!(tx.capacity(), 1);

        // Test interop
        let tokio_tx: tokio::sync::mpsc::Sender<i32> = tx.into();
        tokio_tx.send(44).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), 44);
    }

    #[tokio::test]
    async fn test_unbounded_mpsc_proxy() {
        let (tx, mut rx) = mpsc::unbounded_channel::<i32>();
        tx.send(42).unwrap();
        assert_eq!(rx.recv().await.unwrap(), 42);

        // Test Deref
        assert!(!tx.is_closed());

        // Test interop
        let tokio_tx: tokio::sync::mpsc::UnboundedSender<i32> = tx.into();
        tokio_tx.send(44).unwrap();
        assert_eq!(rx.recv().await.unwrap(), 44);
    }

    #[tokio::test]
    async fn test_broadcast_proxy() {
        let (tx, mut rx1) = broadcast::channel::<i32>(10);
        let mut rx2 = tx.subscribe();

        tx.send(42).unwrap();
        assert_eq!(rx1.recv().await.unwrap(), 42);
        assert_eq!(rx2.recv().await.unwrap(), 42);

        // Test Deref
        assert_eq!(tx.receiver_count(), 2);

        // Test interop
        let tokio_tx: tokio::sync::broadcast::Sender<i32> = tx.into();
        tokio_tx.send(44).unwrap();
        assert_eq!(rx1.recv().await.unwrap(), 44);
        assert_eq!(rx2.recv().await.unwrap(), 44);
    }

    #[tokio::test]
    async fn test_semantic_naming() {
        let (tx, _rx) = mpsc::channel::<SemanticType>(1);
        tx.send(SemanticType).await.unwrap();
        // This test primarily verifies compilation and the tagging pattern works.
        // The actual tracing output is not easily verified in unit tests without a subscriber.
    }
}
