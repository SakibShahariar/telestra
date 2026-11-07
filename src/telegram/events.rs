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

/// One decoded backend event plus the application operation it completes, if any.
#[derive(Debug, PartialEq, Eq)]
pub struct RoutedBackendEvent<T> {
    event: BackendEvent,
    completed_request: Option<(RequestId, T)>,
}

impl<T> RoutedBackendEvent<T> {
    /// Returns the application-facing event in TDLib receive order.
    pub fn event(&self) -> &BackendEvent {
        &self.event
    }

    /// Takes the completed operation, if this event matched a pending request.
    pub fn take_completed_request(&mut self) -> Option<(RequestId, T)> {
        self.completed_request.take()
    }
}

/// Correlates decoded responses with pending application operations.
///
/// Feed messages to this router in the order returned by the single TDLib
/// receive loop. It deliberately does not parse JSON; the adapter's decoder
/// must classify each TDLib object before routing it here.
#[derive(Debug, Default)]
pub struct EventRouter<T> {
    pending: crate::telegram::PendingRequests<T>,
}

impl<T> EventRouter<T> {
    /// Registers an operation before its request is sent to TDLib.
    pub fn register(&mut self, request_id: RequestId, operation: T) -> Option<T> {
        self.pending.insert(request_id, operation)
    }

    /// Routes a response and completes only the operation matching its @extra.
    pub fn route_response(&mut self, extra: Option<&str>, json: String) -> RoutedBackendEvent<T> {
        let request_id = extra.and_then(RequestId::from_extra);
        let completed_request =
            request_id.and_then(|id| self.pending.resolve(id).map(|operation| (id, operation)));
        RoutedBackendEvent {
            event: BackendEvent::Response { request_id, json },
            completed_request,
        }
    }

    /// Routes a TDLib error and completes only the operation matching its @extra.
    pub fn route_error(&mut self, extra: Option<&str>, error: TdlibError) -> RoutedBackendEvent<T> {
        let request_id = extra.and_then(RequestId::from_extra);
        let completed_request =
            request_id.and_then(|id| self.pending.resolve(id).map(|operation| (id, operation)));
        RoutedBackendEvent {
            event: BackendEvent::Error { request_id, error },
            completed_request,
        }
    }

    /// Routes an unsolicited update without consuming a pending request.
    pub fn route_update(&self, json: String) -> RoutedBackendEvent<T> {
        RoutedBackendEvent {
            event: BackendEvent::Update { json },
            completed_request: None,
        }
    }

    /// Routes an authorization-state update without consuming a pending request.
    pub fn route_authorization_state(&self, state: AuthorizationState) -> RoutedBackendEvent<T> {
        RoutedBackendEvent {
            event: BackendEvent::AuthorizationStateChanged(state),
            completed_request: None,
        }
    }

    /// Drains all operations still pending when the worker stops.
    pub fn drain_pending(&mut self) -> std::collections::btree_map::IntoIter<RequestId, T> {
        self.pending.drain()
    }

    /// Returns the number of operations awaiting a response.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthorizationState, BackendEvent, TdlibError};
    use crate::telegram::{RequestId, TransportError, TransportOperation};

    #[test]
    fn event_router_correlates_response_and_preserves_unmatched_operations() {
        use super::EventRouter;

        let first = RequestId::new(31);
        let second = RequestId::new(32);
        let mut router = EventRouter::default();
        router.register(first, "first");
        router.register(second, "second");

        let mut response =
            router.route_response(Some(&second.to_extra()), "{\"@type\":\"ok\"}".to_owned());
        assert_eq!(
            response.event(),
            &BackendEvent::Response {
                request_id: Some(second),
                json: "{\"@type\":\"ok\"}".to_owned(),
            }
        );
        assert_eq!(response.take_completed_request(), Some((second, "second")));
        assert_eq!(router.pending_len(), 1);

        let mut update = router.route_update("{\"@type\":\"updateNewChat\"}".to_owned());
        assert!(matches!(update.event(), BackendEvent::Update { .. }));
        assert_eq!(update.take_completed_request(), None);
        assert_eq!(router.pending_len(), 1);
        assert_eq!(
            router.drain_pending().collect::<Vec<_>>(),
            vec![(first, "first")]
        );
    }

    #[test]
    fn event_router_correlates_tdlib_errors_without_consuming_unknown_ids() {
        use super::EventRouter;

        let request_id = RequestId::new(44);
        let mut router = EventRouter::default();
        router.register(request_id, "pending");

        let mut unknown =
            router.route_error(Some("foreign:44"), TdlibError::new(400, "Invalid request"));
        assert_eq!(
            unknown.event(),
            &BackendEvent::Error {
                request_id: None,
                error: TdlibError::new(400, "Invalid request"),
            }
        );
        assert_eq!(unknown.take_completed_request(), None);
        assert_eq!(router.pending_len(), 1);

        let mut matched = router.route_error(
            Some(&request_id.to_extra()),
            TdlibError::new(400, "Invalid request"),
        );
        assert_eq!(
            matched.take_completed_request(),
            Some((request_id, "pending"))
        );
        assert_eq!(router.pending_len(), 0);
    }

    #[test]
    fn event_router_keeps_unmatched_valid_request_ids_on_events() {
        use super::EventRouter;

        let mut router = EventRouter::<()>::default();
        let event = router.route_response(Some("telestra:999"), "{\"@type\":\"ok\"}".to_owned());
        assert_eq!(
            event.event(),
            &BackendEvent::Response {
                request_id: Some(RequestId::new(999)),
                json: "{\"@type\":\"ok\"}".to_owned(),
            }
        );
        assert_eq!(router.pending_len(), 0);
    }

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
