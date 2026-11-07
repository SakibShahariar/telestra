//! Application-owned events emitted by the Telegram adapter.
//!
//! The JSON decoder will construct these values on the backend worker. Raw
//! payloads are retained only for response/update data that has not yet gained
//! a stable application model; GTK widgets must never parse TDLib JSON.

use crate::telegram::{RequestId, TransportError};

/// A TDLib authorization phase relevant to application lifecycle decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationState {
    WaitTdlibParameters,
    WaitEncryptionKey,
    WaitPhoneNumber,
    WaitCode,
    WaitPassword,
    Ready,
    LoggingOut,
    Closing,
    Closed,
    /// A future TDLib authorization state not yet modeled by this version.
    Unknown(String),
}

impl AuthorizationState {
    /// Returns true only after TDLib confirms that the client is fully closed.
    pub const fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }
}

/// A structured error returned in a TDLib JSON response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdlibError {
    code: i32,
    message: String,
}

impl TdlibError {
    /// Creates an application-owned representation of a TDLib error.
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the TDLib error code.
    pub const fn code(&self) -> i32 {
        self.code
    }

    /// Returns the TDLib error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// One item in the ordered stream from a single TDLib client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendEvent {
    /// A response correlated by the request's `@extra` field, when present.
    Response {
        request_id: Option<RequestId>,
        json: String,
    },
    /// An unsolicited TDLib update. Updates remain in receive order.
    Update { json: String },
    /// A decoded authorization-state transition.
    AuthorizationStateChanged(AuthorizationState),
    /// A TDLib `error` response, correlated when its `@extra` is valid.
    Error {
        request_id: Option<RequestId>,
        error: TdlibError,
    },
    /// The native JSON transport failed and the client can no longer progress.
    TransportFailed(TransportError),
}

#[cfg(test)]
mod tests {
    use super::{AuthorizationState, BackendEvent, TdlibError};
    use crate::telegram::{RequestId, TransportError, TransportOperation};

    #[test]
    fn unknown_authorization_states_are_preserved() {
        let state = AuthorizationState::Unknown("authorizationStateFuture".to_owned());
        assert_eq!(
            state,
            AuthorizationState::Unknown("authorizationStateFuture".to_owned())
        );
    }

    #[test]
    fn response_event_keeps_optional_request_correlation() {
        let event = BackendEvent::Response {
            request_id: Some(RequestId::new(17)),
            json: "{\"@type\":\"ok\"}".to_owned(),
        };

        assert_eq!(
            event,
            BackendEvent::Response {
                request_id: Some(RequestId::new(17)),
                json: "{\"@type\":\"ok\"}".to_owned()
            }
        );
    }

    #[test]
    fn tdlib_errors_keep_code_and_message_separate() {
        let error = TdlibError::new(400, "Invalid request");
        assert_eq!(error.code(), 400);
        assert_eq!(error.message(), "Invalid request");
    }

    #[test]
    fn tdlib_error_event_keeps_request_correlation() {
        let event = BackendEvent::Error {
            request_id: Some(RequestId::new(23)),
            error: TdlibError::new(400, "Invalid request"),
        };

        assert_eq!(
            event,
            BackendEvent::Error {
                request_id: Some(RequestId::new(23)),
                error: TdlibError::new(400, "Invalid request")
            }
        );
    }

    #[test]
    fn transport_failure_can_be_delivered_as_a_backend_event() {
        let event = BackendEvent::TransportFailed(TransportError::new(
            TransportOperation::Receive,
            "native receive failed",
        ));

        assert_eq!(
            event,
            BackendEvent::TransportFailed(TransportError::new(
                TransportOperation::Receive,
                "native receive failed"
            ))
        );
    }

    #[test]
    fn authorization_ready_and_closed_are_distinct_states() {
        assert_ne!(AuthorizationState::Ready, AuthorizationState::Closed);
    }

    #[test]
    fn only_closed_authorization_state_confirms_shutdown() {
        assert!(!AuthorizationState::Closing.is_closed());
        assert!(AuthorizationState::Closed.is_closed());
        assert!(!AuthorizationState::Unknown("future".to_owned()).is_closed());
    }
}
