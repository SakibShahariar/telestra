//! Application-facing Telegram backend types.
//!
//! Keep TDLib-specific requests, update objects and runtime ownership behind
//! this module. The concrete adapter will be added after its build and
//! lifecycle behavior has been verified.

mod events;
mod lifecycle;
mod protocol;
mod transport;

pub use events::{AuthorizationState, BackendEvent, EventRouter, RoutedBackendEvent, TdlibError};
pub use lifecycle::{ClientLifecycle, ClientState, LifecycleError};
pub use protocol::{PendingRequests, RequestId, RequestIdSequence};
pub use transport::{JsonTransport, TransportError, TransportOperation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendStatus {
    NotConfigured,
}

impl BackendStatus {
    pub const fn description(self) -> &'static str {
        match self {
            Self::NotConfigured => {
                "Telegram connectivity is not configured yet. This early preview does not sign in or sync messages."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BackendStatus;

    #[test]
    fn unconfigured_backend_is_described_honestly() {
        assert!(
            BackendStatus::NotConfigured
                .description()
                .contains("does not sign in")
        );
    }
}
