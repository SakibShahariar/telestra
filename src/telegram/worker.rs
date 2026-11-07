//! Single-owner background worker for one JSON TDLib transport.
//!
//! Requests are supplied as complete JSON objects with a Telestra-generated
//! `@extra`. The worker validates that correlation value, registers the
//! operation before sending, and owns the only receive loop for the transport.
//! It never touches GTK; consumers must forward events to the GLib main context.

use super::{
    AuthorizationState, BackendEvent, ClientLifecycle, ClientState, DecodedMessage, EventRouter,
    JsonTransport, RequestId, TransportError, decode_message,
};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Commands accepted by the backend worker.
#[derive(Debug)]
enum WorkerCommand {
    Request {
        request_id: RequestId,
        operation: String,
        json: String,
    },
    Shutdown,
}

/// Events emitted by the backend worker in receive order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerEvent {
    /// A decoded TDLib event and the operation completed by its response.
    Backend {
        event: BackendEvent,
        completed_operation: Option<(RequestId, String)>,
    },
    /// A request was rejected before it reached TDLib.
    RequestRejected {
        request_id: RequestId,
        reason: String,
    },
    /// A request failed while being sent.
    RequestFailed {
        request_id: RequestId,
        operation: String,
        error: TransportError,
    },
    /// An operation remained pending when the worker became unusable.
    PendingFailed {
        request_id: RequestId,
        operation: String,
        reason: String,
    },
    /// The worker stopped; Failed is distinct from a confirmed clean close.
    Stopped(ClientLifecycle),
}

/// A command handle and ordered event receiver for one backend worker.
///
/// The worker owns the transport. This handle is safe to keep on the GTK side,
/// but received events must still be dispatched onto the GLib main context.
pub struct BackendWorker {
    commands: Sender<WorkerCommand>,
    events: Receiver<WorkerEvent>,
    thread: Option<JoinHandle<()>>,
}

impl BackendWorker {
    /// Starts a worker and transfers exclusive transport ownership to it.
    pub fn spawn<T: JsonTransport>(transport: T) -> std::io::Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("telestra-tdlib".to_owned())
            .spawn(move || run_worker(transport, command_rx, event_tx))?;

        Ok(Self {
            commands: command_tx,
            events: event_rx,
            thread: Some(thread),
        })
    }

    /// Queues a request. Its JSON must be an object with the matching
    /// `"@extra":"telestra:<id>"` field.
    ///
    /// The worker registers the operation before calling transport.send.
    pub fn send_request(
        &self,
        request_id: RequestId,
        operation: impl Into<String>,
        json: impl Into<String>,
    ) -> Result<(), mpsc::SendError<()>> {
        self.commands
            .send(WorkerCommand::Request {
                request_id,
                operation: operation.into(),
                json: json.into(),
            })
            .map_err(|_| mpsc::SendError(()))
    }

    /// Requests orderly shutdown. The worker keeps receiving until TDLib
    /// confirms `authorizationStateClosed`.
    pub fn shutdown(&self) -> Result<(), mpsc::SendError<()>> {
        self.commands
            .send(WorkerCommand::Shutdown)
            .map_err(|_| mpsc::SendError(()))
    }

    /// Receives the next worker event in delivery order.
    pub fn events(&self) -> &Receiver<WorkerEvent> {
        &self.events
    }

    /// Waits for the worker thread to finish.
    pub fn join(mut self) -> thread::Result<()> {
        if let Some(thread) = self.thread.take() {
            thread.join()
        } else {
            Ok(())
        }
    }
}

impl Drop for BackendWorker {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Shutdown);
    }
}

fn run_worker<T: JsonTransport>(
    mut transport: T,
    commands: Receiver<WorkerCommand>,
    events: Sender<WorkerEvent>,
) {
    const RECEIVE_TIMEOUT: Duration = Duration::from_millis(20);

    let mut lifecycle = ClientState::new();
    let _ = lifecycle.start();
    let mut router = EventRouter::<String>::default();
    let mut shutdown_requested = false;

    while !lifecycle.is_terminal() {
        if !shutdown_requested {
            match commands.recv_timeout(RECEIVE_TIMEOUT) {
                Ok(WorkerCommand::Request {
                    request_id,
                    operation,
                    json,
                }) => {
                    let expected_extra = request_id.to_extra();
                    let valid = matches!(
                        decode_message(&json),
                        Ok(DecodedMessage::Response { extra: Some(extra), .. })
                            if extra == expected_extra
                    );
                    if !valid {
                        let _ = events.send(WorkerEvent::RequestRejected {
                            request_id,
                            reason: "request must be a valid TDLib JSON object with its matching Telestra @extra".to_owned(),
                        });
                        continue;
                    }

                    if router.contains(request_id) {
                        let _ = events.send(WorkerEvent::RequestRejected {
                            request_id,
                            reason: "request ID is already pending".to_owned(),
                        });
                        continue;
                    }

                    router.register(request_id, operation.clone());
                    if let Err(error) = transport.send(&json) {
                        let _ = router.cancel(request_id);
                        let _ = events.send(WorkerEvent::RequestFailed {
                            request_id,
                            operation,
                            error: error.clone(),
                        });
                        fail_worker(&mut lifecycle, &mut router, &events, error.to_string());
                        break;
                    }
                }
                Ok(WorkerCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    shutdown_requested = true;
                    if lifecycle.begin_close().is_err() {
                        lifecycle.fail();
                        break;
                    }
                    if let Err(error) = transport.close() {
                        emit_transport_failure(&mut lifecycle, &mut router, &events, error);
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }

        match transport.receive(RECEIVE_TIMEOUT) {
            Ok(Some(json)) => match decode_message(&json) {
                Ok(message) => {
                    let closed = matches!(
                        &message,
                        DecodedMessage::AuthorizationStateChanged(AuthorizationState::Closed)
                    );
                    let mut routed = router.route_decoded_message(message);
                    let event = routed.event().clone();
                    let completed_operation = routed.take_completed_request();
                    let _ = events.send(WorkerEvent::Backend {
                        event,
                        completed_operation,
                    });

                    if shutdown_requested
                        && closed
                        && lifecycle
                            .observe_authorization_state(&AuthorizationState::Closed)
                            .is_err()
                    {
                        lifecycle.fail();
                    }
                }
                Err(error) => {
                    fail_worker(&mut lifecycle, &mut router, &events, error.to_string());
                    break;
                }
            },
            Ok(None) => {}
            Err(error) => {
                emit_transport_failure(&mut lifecycle, &mut router, &events, error);
                break;
            }
        }
    }

    let _ = events.send(WorkerEvent::Stopped(lifecycle.lifecycle()));
}

fn emit_transport_failure(
    lifecycle: &mut ClientState,
    router: &mut EventRouter<String>,
    events: &Sender<WorkerEvent>,
    error: TransportError,
) {
    let _ = events.send(WorkerEvent::Backend {
        event: BackendEvent::TransportFailed(error.clone()),
        completed_operation: None,
    });
    fail_worker(lifecycle, router, events, error.to_string());
}

fn fail_worker(
    lifecycle: &mut ClientState,
    router: &mut EventRouter<String>,
    events: &Sender<WorkerEvent>,
    reason: String,
) {
    lifecycle.fail();
    for (request_id, operation) in router.drain_pending() {
        let _ = events.send(WorkerEvent::PendingFailed {
            request_id,
            operation,
            reason: reason.clone(),
        });
    }
}
