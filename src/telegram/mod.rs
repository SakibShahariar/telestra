//! Application-facing Telegram backend types.
//!
//! Keep TDLib-specific requests, update objects and runtime ownership behind
//! this module. Transport-independent JSON decoding, event routing, request
//! correlation, lifecycle primitives and a transport-owning background worker
//! are implemented here. The native TDLib C ABI loader and mailbox adapters
//! are available, including a tested GLib main-context event bridge;
//! application startup composition and account workflows are not yet connected.

mod decoder;
mod dispatcher;
mod events;
mod lifecycle;
mod protocol;
mod transfers;
mod transport;
mod worker;

pub use tdlib_ffi::{TdJsonError, TdJsonLibrary};

pub use decoder::{
    DecodeError, DecodedEnvelope, DecodedMessage, decode_envelope, decode_message,
    decode_transfer_list_page, decode_transfer_update,
};
pub use dispatcher::{
    AccountMailboxTransport, ClientMailboxRegistry, ClientRequestSender, DispatchError,
    NativeTdLibReceiveSource, NativeTdLibRequestSender, SharedDispatcher, SharedReceiveSource,
};
pub use events::{AuthorizationState, BackendEvent, EventRouter, RoutedBackendEvent, TdlibError};
pub use lifecycle::{ClientLifecycle, ClientState, LifecycleError};
pub use protocol::{PendingRequests, RequestId, RequestIdSequence};
pub use transfers::{
    DownloadListEntry, DownloadedFileCounts, FileTransferState, TransferCommand, TransferItem,
    TransferListPage, TransferProgress, TransferQueue,
    TransferQueueError, TransferStatus, TransferUpdate,
};
pub use transport::{JsonTransport, TransportError, TransportOperation};
pub use worker::{BackendWorker, WorkerEvent};

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
