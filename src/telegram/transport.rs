//! A narrow, testable boundary for TDLib's asynchronous JSON transport.
//!
//! The concrete implementation will wrap the client-scoped
//! `td_json_client_*` C API. This trait deliberately models one owned client:
//! mutable access to `receive` prevents callers from polling the same client
//! concurrently, and each returned message is an owned Rust string.

use std::fmt;
use std::time::Duration;

/// The operation that failed at the native transport boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportOperation {
    Send,
    Receive,
    Close,
}

/// An error reported by a JSON transport implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    operation: TransportOperation,
    message: String,
}

impl TransportError {
    /// Creates an error while retaining which transport operation failed.
    pub fn new(operation: TransportOperation, message: impl Into<String>) -> Self {
        Self {
            operation,
            message: message.into(),
        }
    }

    /// Returns the failed transport operation.
    pub const fn operation(&self) -> TransportOperation {
        self.operation
    }

    /// Returns the diagnostic message supplied by the transport.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?} failed: {}", self.operation, self.message)
    }
}

impl std::error::Error for TransportError {}

/// One client-scoped TDLib JSON transport.
///
/// Implementations must preserve TDLib's message order. `receive` must return
/// an owned string because TDLib's C API invalidates its borrowed response
/// pointer on a later receive/execute call. A runtime must have exactly one
/// receive-loop owner per client.
pub trait JsonTransport: Send + 'static {
    /// Sends one complete JSON request to the owned TDLib client.
    fn send(&mut self, request: &str) -> Result<(), TransportError>;

    /// Receives the next message, waiting no longer than `timeout`.
    ///
    /// `Ok(None)` means the timeout elapsed without a message; it is not a
    /// transport failure and does not indicate that the client has closed.
    fn receive(&mut self, timeout: Duration) -> Result<Option<String>, TransportError>;

    /// Begins orderly closure of the owned client.
    ///
    /// A TDLib implementation must not destroy the native client here. It
    /// should send the TDLib `close` request and let its owner continue
    /// receiving until TDLib reports `authorizationStateClosed`.
    fn close(&mut self) -> Result<(), TransportError>;
}

#[cfg(test)]
mod tests {
    use super::{JsonTransport, TransportError, TransportOperation};
    use std::collections::VecDeque;
    use std::time::Duration;

    #[derive(Default)]
    struct ScriptedTransport {
        incoming: VecDeque<String>,
        sent: Vec<String>,
        close_requested: bool,
    }

    impl JsonTransport for ScriptedTransport {
        fn send(&mut self, request: &str) -> Result<(), TransportError> {
            self.sent.push(request.to_owned());
            Ok(())
        }

        fn receive(&mut self, _timeout: Duration) -> Result<Option<String>, TransportError> {
            Ok(self.incoming.pop_front())
        }

        fn close(&mut self) -> Result<(), TransportError> {
            self.close_requested = true;
            Ok(())
        }
    }

    #[test]
    fn transport_preserves_incoming_message_order() {
        let mut transport = ScriptedTransport {
            incoming: ["first", "second", "third"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ..Default::default()
        };

        for expected in ["first", "second", "third"] {
            assert_eq!(
                transport.receive(Duration::ZERO).unwrap().as_deref(),
                Some(expected)
            );
        }
        assert_eq!(transport.receive(Duration::ZERO).unwrap(), None);
    }

    #[test]
    fn transport_records_requests_in_send_order() {
        let mut transport = ScriptedTransport::default();

        transport.send("{\"@type\":\"getMe\"}").unwrap();
        transport.send("{\"@type\":\"getChats\"}").unwrap();

        assert_eq!(
            transport.sent,
            vec![
                "{\"@type\":\"getMe\"}",
                "{\"@type\":\"getChats\"}"
            ]
        );
    }

    #[test]
    fn transport_close_is_explicit_and_does_not_discard_messages() {
        let mut transport = ScriptedTransport {
            incoming: std::iter::once("authorizationStateClosing".to_owned()).collect(),
            ..Default::default()
        };

        transport.close().unwrap();

        assert!(transport.close_requested);
        assert_eq!(
            transport.receive(Duration::ZERO).unwrap().as_deref(),
            Some("authorizationStateClosing")
        );
    }

    #[test]
    fn transport_errors_keep_the_failed_operation() {
        let error = TransportError::new(TransportOperation::Receive, "native receive failed");

        assert_eq!(error.operation(), TransportOperation::Receive);
        assert_eq!(error.message(), "native receive failed");
        assert_eq!(error.to_string(), "Receive failed: native receive failed");
    }
}
