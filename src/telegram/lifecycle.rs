//! Explicit state for a single TDLib client's lifetime.
//!
//! This state machine does not call TDLib itself. The runtime owner uses it to
//! reject invalid transitions and, in particular, to distinguish requesting
//! shutdown from TDLib confirming that its authorization state is closed.

use super::events::AuthorizationState;
use std::fmt;

/// Observable lifecycle phase for one owned TDLib client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientLifecycle {
    #[default]
    Created,
    Running,
    Closing,
    Closed,
    Failed,
}

/// A rejected transition, including the phase in which it was attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleError {
    current: ClientLifecycle,
    operation: &'static str,
}

impl LifecycleError {
    /// Returns the phase that prevented the requested transition.
    pub const fn current(self) -> ClientLifecycle {
        self.current
    }

    /// Returns the attempted transition.
    pub const fn operation(self) -> &'static str {
        self.operation
    }
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot {} while TDLib client is {:?}",
            self.operation, self.current
        )
    }
}

impl std::error::Error for LifecycleError {}

/// Tracks the lifecycle of exactly one transport/client instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClientState {
    lifecycle: ClientLifecycle,
}

impl ClientState {
    /// Creates a state machine for a newly-created client.
    pub const fn new() -> Self {
        Self {
            lifecycle: ClientLifecycle::Created,
        }
    }

    /// Returns the current lifecycle phase.
    pub const fn lifecycle(self) -> ClientLifecycle {
        self.lifecycle
    }

    /// Marks the client ready for requests and updates.
    pub fn start(&mut self) -> Result<(), LifecycleError> {
        self.transition(ClientLifecycle::Created, ClientLifecycle::Running, "start")
    }

    /// Records that an orderly TDLib close request has been sent.
    ///
    /// This does not mean the native client may be destroyed yet. The receive
    /// loop must continue until TDLib reports `authorizationStateClosed`.
    pub fn begin_close(&mut self) -> Result<(), LifecycleError> {
        self.transition(
            ClientLifecycle::Running,
            ClientLifecycle::Closing,
            "begin close",
        )
    }

    /// Records TDLib's `authorizationStateClosed` confirmation.
    pub fn confirm_closed(&mut self) -> Result<(), LifecycleError> {
        self.transition(
            ClientLifecycle::Closing,
            ClientLifecycle::Closed,
            "confirm closed",
        )
    }

    /// Applies an authorization-state update to the client lifecycle.
    ///
    /// Intermediate states do not end the receive loop. Only TDLib's explicit
    /// `authorizationStateClosed` confirmation completes an orderly close.
    pub fn observe_authorization_state(
        &mut self,
        state: &AuthorizationState,
    ) -> Result<(), LifecycleError> {
        if state.is_closed() {
            self.confirm_closed()
        } else {
            Ok(())
        }
    }

    /// Marks the client unusable after an unrecoverable transport/runtime error.
    pub fn fail(&mut self) {
        if self.lifecycle != ClientLifecycle::Closed {
            self.lifecycle = ClientLifecycle::Failed;
        }
    }

    /// Returns whether the runtime should stop its receive loop.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self.lifecycle,
            ClientLifecycle::Closed | ClientLifecycle::Failed
        )
    }

    fn transition(
        &mut self,
        expected: ClientLifecycle,
        next: ClientLifecycle,
        operation: &'static str,
    ) -> Result<(), LifecycleError> {
        if self.lifecycle != expected {
            return Err(LifecycleError {
                current: self.lifecycle,
                operation,
            });
        }
        self.lifecycle = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientLifecycle, ClientState};

    #[test]
    fn client_must_confirm_closed_after_beginning_shutdown() {
        let mut state = ClientState::new();

        assert_eq!(state.lifecycle(), ClientLifecycle::Created);
        assert!(!state.is_terminal());
        state.start().unwrap();
        state.begin_close().unwrap();

        assert_eq!(state.lifecycle(), ClientLifecycle::Closing);
        assert!(!state.is_terminal());

        state.confirm_closed().unwrap();
        assert_eq!(state.lifecycle(), ClientLifecycle::Closed);
        assert!(state.is_terminal());
    }

    #[test]
    fn client_cannot_be_closed_before_tdlib_confirms_it() {
        let mut state = ClientState::new();
        state.start().unwrap();

        let error = state.confirm_closed().unwrap_err();
        assert_eq!(error.current(), ClientLifecycle::Running);
        assert_eq!(error.operation(), "confirm closed");
        assert_eq!(
            error.to_string(),
            "cannot confirm closed while TDLib client is Running"
        );
        assert_eq!(state.lifecycle(), ClientLifecycle::Running);
    }

    #[test]
    fn client_cannot_restart_or_close_twice() {
        let mut state = ClientState::new();
        state.start().unwrap();
        state.begin_close().unwrap();
        state.confirm_closed().unwrap();

        assert!(state.start().is_err());
        assert!(state.begin_close().is_err());
        assert_eq!(state.lifecycle(), ClientLifecycle::Closed);
    }

    #[test]
    fn runtime_failure_is_terminal_but_cannot_reopen_a_closed_client() {
        let mut failed = ClientState::new();
        failed.fail();
        assert_eq!(failed.lifecycle(), ClientLifecycle::Failed);
        assert!(failed.is_terminal());

        let mut closed = ClientState::new();
        closed.start().unwrap();
        closed.begin_close().unwrap();
        closed.confirm_closed().unwrap();
        closed.fail();
        assert_eq!(closed.lifecycle(), ClientLifecycle::Closed);
    }

    #[test]
    fn failure_during_close_is_not_mistaken_for_clean_shutdown() {
        let mut state = ClientState::new();
        state.start().unwrap();
        state.begin_close().unwrap();

        state.fail();

        assert_eq!(state.lifecycle(), ClientLifecycle::Failed);
        assert!(state.is_terminal());
        assert_ne!(state.lifecycle(), ClientLifecycle::Closed);
    }

    #[test]
    fn only_tdlib_closed_authorization_update_completes_shutdown() {
        use super::super::events::AuthorizationState;

        let mut state = ClientState::new();
        state.start().unwrap();
        state.begin_close().unwrap();

        state
            .observe_authorization_state(&AuthorizationState::Closing)
            .unwrap();
        assert_eq!(state.lifecycle(), ClientLifecycle::Closing);

        state
            .observe_authorization_state(&AuthorizationState::Closed)
            .unwrap();
        assert_eq!(state.lifecycle(), ClientLifecycle::Closed);
    }

    #[test]
    fn default_client_state_starts_in_created_phase() {
        let state = ClientState::default();

        assert_eq!(state.lifecycle(), ClientLifecycle::Created);
        assert!(!state.is_terminal());
    }
}
