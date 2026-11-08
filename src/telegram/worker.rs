//! Single-owner background worker for one JSON TDLib transport.
//!
//! Requests are supplied as complete JSON objects with a Telestra-generated
//! `@extra`. The worker validates that correlation value, registers the
//! operation before sending, and owns the only receive loop for the transport.
//! It never touches GTK; consumers must forward events to the GLib main context.

use super::{
    AuthorizationState, BackendEvent, ClientLifecycle, ClientState, DecodedMessage, EventRouter,
    JsonTransport, RequestId, TransferCommand, TransferListPage, TransportError, decode_message,
};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
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
    /// A received message could not be decoded as a TDLib JSON envelope.
    ProtocolFailed { message: String },
    /// An operation remained pending when the worker became unusable or closed.
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
    events: Option<Receiver<WorkerEvent>>,
    thread: Option<JoinHandle<()>>,
    shutdown_requested: Mutex<bool>,
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
            events: Some(event_rx),
            thread: Some(thread),
            shutdown_requested: Mutex::new(false),
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
        let shutdown_requested = self.shutdown_requested.lock().unwrap();
        if *shutdown_requested {
            return Err(mpsc::SendError(()));
        }
        self.commands
            .send(WorkerCommand::Request {
                request_id,
                operation: operation.into(),
                json: json.into(),
            })
            .map_err(|_| mpsc::SendError(()))
    }

    /// Sends a typed download-queue intent through the same correlated worker path.
    pub fn send_transfer_command(
        &self,
        request_id: RequestId,
        command: &TransferCommand,
    ) -> Result<(), mpsc::SendError<()>> {
        self.send_request(
            request_id,
            command.operation_name(),
            command.to_tdlib_json(request_id),
        )
    }

    /// Requests one page of TDLib's persistent message-attachment download list.
    ///
    /// The returned cursor is opaque and must be passed unchanged to the next
    /// page request. Callers own pagination and must reconcile live updates
    /// received while the initial list is being hydrated.
    pub fn search_transfer_list_page(
        &self,
        request_id: RequestId,
        offset: &str,
    ) -> Result<(), mpsc::SendError<()>> {
        self.send_request(
            request_id,
            "searchFileDownloads",
            TransferListPage::search_request_json(request_id, offset),
        )
    }

    /// Requests orderly shutdown. The worker keeps receiving until TDLib
    /// confirms `authorizationStateClosed`.
    pub fn shutdown(&self) -> Result<(), mpsc::SendError<()>> {
        let mut shutdown_requested = self.shutdown_requested.lock().unwrap();
        if *shutdown_requested {
            return Err(mpsc::SendError(()));
        }
        *shutdown_requested = true;
        self.commands
            .send(WorkerCommand::Shutdown)
            .map_err(|_| mpsc::SendError(()))
    }

    /// Receives the next worker event in delivery order.
    ///
    /// This receiver is unavailable after `attach_events` transfers it to
    /// the GLib main-context bridge.
    pub fn events(&self) -> &Receiver<WorkerEvent> {
        self.events
            .as_ref()
            .expect("worker events were moved to the GLib main context")
    }

    /// Attaches worker events to the GLib default main context.
    ///
    /// Call this from the thread that owns the main context. The callback runs
    /// on that context (and may update GTK); the backend thread never touches
    /// UI objects. At most 128 queued events are handled per 16 ms tick so a
    /// busy backend cannot starve other main-loop sources.
    pub fn attach_events<F>(&mut self, callback: F) -> Option<glib::SourceId>
    where
        F: FnMut(WorkerEvent) + 'static,
    {
        let receiver = self.events.take()?;
        Some(attach_event_receiver(receiver, callback))
    }

    /// Waits for the worker thread to finish.
    pub fn join(mut self) -> thread::Result<()> {
        let _ = self.shutdown();
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

fn attach_event_receiver<F>(receiver: Receiver<WorkerEvent>, mut callback: F) -> glib::SourceId
where
    F: FnMut(WorkerEvent) + 'static,
{
    glib::timeout_add_local(Duration::from_millis(16), move || {
        for _ in 0..128 {
            match receiver.try_recv() {
                Ok(event) => callback(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
        glib::ControlFlow::Continue
    })
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

                    if shutdown_requested && closed {
                        if lifecycle
                            .observe_authorization_state(&AuthorizationState::Closed)
                            .is_err()
                        {
                            lifecycle.fail();
                        } else {
                            fail_pending(
                                &mut router,
                                &events,
                                "TDLib closed before the operation received a response",
                            );
                        }
                    }
                }
                Err(error) => {
                    let reason = error.to_string();
                    let _ = events.send(WorkerEvent::ProtocolFailed {
                        message: reason.clone(),
                    });
                    fail_worker(&mut lifecycle, &mut router, &events, reason);
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
    fail_pending(router, events, &reason);
}

fn fail_pending(router: &mut EventRouter<String>, events: &Sender<WorkerEvent>, reason: &str) {
    for (request_id, operation) in router.drain_pending() {
        let _ = events.send(WorkerEvent::PendingFailed {
            request_id,
            operation,
            reason: reason.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{BackendWorker, WorkerEvent};
    use crate::telegram::{
        AuthorizationState, ClientLifecycle, JsonTransport, RequestId, TransportError,
        TransportOperation,
    };
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct FakeTransport {
        incoming: VecDeque<String>,
        sent: Arc<Mutex<Vec<String>>>,
    }

    impl JsonTransport for FakeTransport {
        fn send(&mut self, request: &str) -> Result<(), TransportError> {
            self.sent.lock().unwrap().push(request.to_owned());
            Ok(())
        }

        fn receive(&mut self, _timeout: Duration) -> Result<Option<String>, TransportError> {
            Ok(self.incoming.pop_front())
        }

        fn close(&mut self) -> Result<(), TransportError> {
            self.incoming.push_back(
                r#"{"@type":"updateAuthorizationState","authorization_state":{"@type":"authorizationStateClosed"}}"#
                    .to_owned(),
            );
            Ok(())
        }
    }

    #[test]
    fn worker_correlates_response_and_waits_for_close_confirmation() {
        let request_id = RequestId::new(73);
        let response = format!(r#"{{"@type":"ok","@extra":"{}"}}"#, request_id.to_extra());
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = FakeTransport {
            incoming: VecDeque::from([response.clone()]),
            sent: Arc::clone(&sent),
        };
        let worker = BackendWorker::spawn(transport).unwrap();

        let request = format!(
            r#"{{"@type":"getMe","@extra":"{}"}}"#,
            request_id.to_extra()
        );
        worker
            .send_request(request_id, "load-current-user", request.clone())
            .unwrap();

        let response_event = worker
            .events()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            response_event,
            WorkerEvent::Backend {
                event: crate::telegram::BackendEvent::Response {
                    request_id: Some(request_id),
                    json: response,
                },
                completed_operation: Some((request_id, "load-current-user".to_owned())),
            }
        );
        assert_eq!(*sent.lock().unwrap(), vec![request]);

        worker.shutdown().unwrap();
        let close_event = worker
            .events()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            close_event,
            WorkerEvent::Backend {
                event: crate::telegram::BackendEvent::AuthorizationStateChanged(
                    AuthorizationState::Closed
                ),
                completed_operation: None,
            }
        );
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Closed)
        );
        worker.join().unwrap();
    }

    #[test]
    fn worker_sends_correlated_transfer_list_page_requests() {
        let request_id = RequestId::new(80);
        let response = format!(r#"{{"@type":"foundFileDownloads","total_counts":{{"active_count":0,"paused_count":0,"completed_count":0}},"files":[],"next_offset":"","@extra":"{}"}}"#, request_id.to_extra());
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = FakeTransport {
            incoming: VecDeque::from([response]),
            sent: Arc::clone(&sent),
        };
        let worker = BackendWorker::spawn(transport).unwrap();
        let offset = r#"opaque "cursor"\\page"#;

        worker
            .search_transfer_list_page(request_id, offset)
            .unwrap();

        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Backend {
                completed_operation: Some((id, operation)),
                event: crate::telegram::BackendEvent::Response { .. },
            } if id == request_id && operation == "searchFileDownloads"
        ));
        assert_eq!(
            *sent.lock().unwrap(),
            vec![TransferListPage::search_request_json(request_id, offset)]
        );

        worker.shutdown().unwrap();
        worker.events().recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn worker_sends_transfer_commands_with_request_correlation() {
        use crate::telegram::TransferCommand;

        let request_id = RequestId::new(79);
        let response = format!(r#"{{"@type":"ok","@extra":"{}"}}"#, request_id.to_extra());
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = FakeTransport {
            incoming: VecDeque::from([response]),
            sent: Arc::clone(&sent),
        };
        let worker = BackendWorker::spawn(transport).unwrap();
        let command = TransferCommand::AddToDownloads {
            file_id: 42,
            chat_id: -100,
            message_id: 9,
            priority: 16,
        };

        worker.send_transfer_command(request_id, &command).unwrap();

        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Backend {
                completed_operation: Some((id, operation)),
                ..
            } if id == request_id && operation == "addFileToDownloads"
        ));
        assert_eq!(
            *sent.lock().unwrap(),
            vec![command.to_tdlib_json(request_id)]
        );

        worker.shutdown().unwrap();
        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Backend {
                event: crate::telegram::BackendEvent::AuthorizationStateChanged(
                    AuthorizationState::Closed
                ),
                ..
            }
        ));
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Closed)
        );
        worker.join().unwrap();
    }

    #[test]
    fn worker_rejects_mismatched_correlation_before_sending() {
        let request_id = RequestId::new(74);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = FakeTransport {
            incoming: VecDeque::new(),
            sent: Arc::clone(&sent),
        };
        let worker = BackendWorker::spawn(transport).unwrap();

        worker
            .send_request(
                request_id,
                "invalid-request",
                r#"{"@type":"getMe","@extra":"telestra:999"}"#,
            )
            .unwrap();
        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::RequestRejected {
                request_id: rejected,
                ..
            } if rejected == request_id
        ));
        assert!(sent.lock().unwrap().is_empty());

        worker.shutdown().unwrap();
        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Backend {
                event: crate::telegram::BackendEvent::AuthorizationStateChanged(
                    AuthorizationState::Closed
                ),
                ..
            }
        ));
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Closed)
        );
        worker.join().unwrap();
    }

    #[test]
    fn worker_fails_pending_operations_when_tdlib_closes() {
        let request_id = RequestId::new(75);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = FakeTransport {
            incoming: VecDeque::new(),
            sent: Arc::clone(&sent),
        };
        let worker = BackendWorker::spawn(transport).unwrap();
        let request = format!(
            r#"{{"@type":"getChats","@extra":"{}"}}"#,
            request_id.to_extra()
        );
        worker
            .send_request(request_id, "load-chats", request)
            .unwrap();
        worker.shutdown().unwrap();

        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Backend {
                event: crate::telegram::BackendEvent::AuthorizationStateChanged(
                    AuthorizationState::Closed
                ),
                ..
            }
        ));
        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::PendingFailed {
                request_id: failed,
                operation,
                ..
            } if failed == request_id && operation == "load-chats"
        ));
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Closed)
        );
        assert_eq!(sent.lock().unwrap().len(), 1);
        worker.join().unwrap();
    }

    #[test]
    fn worker_does_not_accept_requests_after_shutdown_is_queued() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = FakeTransport {
            incoming: VecDeque::new(),
            sent,
        };
        let worker = BackendWorker::spawn(transport).unwrap();
        worker.shutdown().unwrap();
        assert!(
            worker
                .send_request(
                    RequestId::new(76),
                    "late-request",
                    r#"{"@type":"getMe","@extra":"telestra:76"}"#,
                )
                .is_err()
        );
        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Backend {
                event: crate::telegram::BackendEvent::AuthorizationStateChanged(
                    AuthorizationState::Closed
                ),
                ..
            }
        ));
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Closed)
        );
        worker.join().unwrap();
    }
    #[test]
    fn worker_surfaces_receive_failure_and_stops_as_failed() {
        struct FailingTransport;

        impl JsonTransport for FailingTransport {
            fn send(&mut self, _request: &str) -> Result<(), TransportError> {
                Ok(())
            }

            fn receive(&mut self, _timeout: Duration) -> Result<Option<String>, TransportError> {
                Err(TransportError::new(
                    TransportOperation::Receive,
                    "scripted receive failure",
                ))
            }

            fn close(&mut self) -> Result<(), TransportError> {
                Ok(())
            }
        }

        let worker = BackendWorker::spawn(FailingTransport).unwrap();
        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Backend {
                event: crate::telegram::BackendEvent::TransportFailed(_),
                ..
            }
        ));
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Failed)
        );
        worker.join().unwrap();
    }

    #[test]
    fn worker_reports_send_failure_and_stops_as_failed() {
        struct FailingSendTransport;

        impl JsonTransport for FailingSendTransport {
            fn send(&mut self, _request: &str) -> Result<(), TransportError> {
                Err(TransportError::new(
                    TransportOperation::Send,
                    "scripted send failure",
                ))
            }

            fn receive(&mut self, _timeout: Duration) -> Result<Option<String>, TransportError> {
                Ok(None)
            }

            fn close(&mut self) -> Result<(), TransportError> {
                Ok(())
            }
        }

        let request_id = RequestId::new(77);
        let worker = BackendWorker::spawn(FailingSendTransport).unwrap();
        let request = format!(
            r#"{{"@type":"getMe","@extra":"{}"}}"#,
            request_id.to_extra()
        );
        worker
            .send_request(request_id, "load-current-user", request)
            .unwrap();

        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::RequestFailed {
                request_id: failed,
                operation,
                error,
            } if failed == request_id
                && operation == "load-current-user"
                && error.operation() == TransportOperation::Send
        ));
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Failed)
        );
        worker.join().unwrap();
    }

    #[test]
    fn worker_reports_malformed_incoming_json_and_fails_pending_work() {
        struct MalformedTransport {
            delivered: bool,
        }

        impl JsonTransport for MalformedTransport {
            fn send(&mut self, _request: &str) -> Result<(), TransportError> {
                Ok(())
            }

            fn receive(&mut self, _timeout: Duration) -> Result<Option<String>, TransportError> {
                if self.delivered {
                    Ok(None)
                } else {
                    self.delivered = true;
                    Ok(Some(r#"{"@type":"ok","broken":}"#.to_owned()))
                }
            }

            fn close(&mut self) -> Result<(), TransportError> {
                Ok(())
            }
        }

        let request_id = RequestId::new(78);
        let worker = BackendWorker::spawn(MalformedTransport { delivered: false }).unwrap();
        let request = format!(
            r#"{{"@type":"getMe","@extra":"{}"}}"#,
            request_id.to_extra()
        );
        worker
            .send_request(request_id, "load-current-user", request)
            .unwrap();

        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::ProtocolFailed { .. }
        ));
        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::PendingFailed {
                request_id: failed,
                operation,
                ..
            } if failed == request_id && operation == "load-current-user"
        ));
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Failed)
        );
        worker.join().unwrap();
    }

    #[test]
    fn worker_reports_protocol_failure_and_stops_as_failed() {
        struct MalformedTransport;

        impl JsonTransport for MalformedTransport {
            fn send(&mut self, _request: &str) -> Result<(), TransportError> {
                Ok(())
            }

            fn receive(&mut self, _timeout: Duration) -> Result<Option<String>, TransportError> {
                Ok(Some(r#"{"@type":"updateNewChat","chat":}"#.to_owned()))
            }

            fn close(&mut self) -> Result<(), TransportError> {
                Ok(())
            }
        }

        let worker = BackendWorker::spawn(MalformedTransport).unwrap();
        assert!(matches!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::ProtocolFailed { .. }
        ));
        assert_eq!(
            worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            WorkerEvent::Stopped(ClientLifecycle::Failed)
        );
        worker.join().unwrap();
    }

    #[test]
    fn attached_events_are_delivered_on_the_glib_main_context() {
        use super::{WorkerEvent, attach_event_receiver};
        use crate::telegram::ClientLifecycle;
        use std::cell::Cell;
        use std::rc::Rc;

        let context = glib::MainContext::default();
        let _context_guard = context.acquire().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let main_thread = std::thread::current().id();
        let observed = Rc::new(Cell::new(false));
        let observed_in_callback = Rc::clone(&observed);

        let _source = attach_event_receiver(receiver, move |event| {
            assert_eq!(std::thread::current().id(), main_thread);
            observed_in_callback.set(event == WorkerEvent::Stopped(ClientLifecycle::Closed));
        });

        std::thread::spawn(move || {
            sender
                .send(WorkerEvent::Stopped(ClientLifecycle::Closed))
                .unwrap();
        })
        .join()
        .unwrap();

        while !observed.get() {
            context.iteration(true);
        }
        assert!(observed.get());
    }
}
