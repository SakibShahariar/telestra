//! Per-account mailboxes for TDLib's process-wide JSON receive stream.
//!
//! The native dispatcher owns this registry and is the only component that
//! calls TDLib's global td_receive. It routes each owned JSON message by the
//! @client_id field before account workers consume their mailboxes.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::{
    DecodeError, JsonTransport, TransportError, TransportOperation, decode_envelope,
};

/// A failure while routing a message from TDLib's shared receive stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// The incoming JSON object did not contain a valid TDLib client ID.
    MissingClientId,
    /// The message referenced a client that has no registered mailbox.
    UnknownClient(i32),
    /// A client ID already has a mailbox registered.
    DuplicateClient(i32),
    /// The account worker has stopped receiving messages.
    MailboxDisconnected(i32),
    /// The incoming TDLib JSON envelope was malformed.
    Decode(DecodeError),
    /// The shared native receive operation failed.
    ReceiveFailed(String),
    /// The dispatcher thread is no longer available.
    DispatcherStopped,
}

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingClientId => formatter.write_str("TDLib message has no @client_id"),
            Self::UnknownClient(client_id) => {
                write!(
                    formatter,
                    "no mailbox registered for TDLib client {client_id}"
                )
            }
            Self::DuplicateClient(client_id) => {
                write!(formatter, "TDLib client {client_id} already has a mailbox")
            }
            Self::MailboxDisconnected(client_id) => {
                write!(
                    formatter,
                    "mailbox for TDLib client {client_id} is disconnected"
                )
            }
            Self::Decode(error) => write!(formatter, "invalid TDLib message: {error}"),
            Self::ReceiveFailed(message) => {
                write!(formatter, "TDLib shared receive failed: {message}")
            }
            Self::DispatcherStopped => formatter.write_str("TDLib dispatcher has stopped"),
        }
    }
}

impl std::error::Error for DispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            _ => None,
        }
    }
}

/// Routes the process-wide TDLib stream into one ordered mailbox per client.
///
/// This registry is intentionally not internally synchronized: it belongs to
/// the single dispatcher thread. Registration and removal should be performed
/// by that thread in response to its control messages.
#[derive(Debug, Default)]
pub struct ClientMailboxRegistry {
    mailboxes: BTreeMap<i32, Sender<String>>,
}

impl ClientMailboxRegistry {
    /// Registers a client and returns the receiver owned by its account worker.
    pub fn register(&mut self, client_id: i32) -> Result<Receiver<String>, DispatchError> {
        match self.mailboxes.entry(client_id) {
            std::collections::btree_map::Entry::Occupied(_) => {
                Err(DispatchError::DuplicateClient(client_id))
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                let (sender, receiver) = mpsc::channel();
                entry.insert(sender);
                Ok(receiver)
            }
        }
    }

    /// Routes an owned TDLib JSON message using its @client_id field.
    ///
    /// The original JSON is forwarded unchanged so the account worker can
    /// decode and process it without losing fields unknown to this version.
    pub fn route_json(&self, json: String) -> Result<(), DispatchError> {
        let envelope = decode_envelope(&json).map_err(DispatchError::Decode)?;
        let client_id = envelope.client_id.ok_or(DispatchError::MissingClientId)?;
        let mailbox = self
            .mailboxes
            .get(&client_id)
            .ok_or(DispatchError::UnknownClient(client_id))?;
        mailbox
            .send(json)
            .map_err(|_| DispatchError::MailboxDisconnected(client_id))
    }

    /// Removes a client mailbox after its closed authorization state arrives.
    pub fn unregister(&mut self, client_id: i32) -> bool {
        self.mailboxes.remove(&client_id).is_some()
    }

    /// Returns whether the client currently has a registered mailbox.
    pub fn contains(&self, client_id: i32) -> bool {
        self.mailboxes.contains_key(&client_id)
    }

    /// Returns the number of registered client mailboxes.
    pub fn len(&self) -> usize {
        self.mailboxes.len()
    }

    /// Returns whether no client mailboxes are registered.
    pub fn is_empty(&self) -> bool {
        self.mailboxes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientMailboxRegistry, DispatchError};

    #[test]
    fn routes_messages_to_the_matching_client_mailbox_in_order() {
        let mut registry = ClientMailboxRegistry::default();
        let first = registry.register(11).unwrap();
        let second = registry.register(12).unwrap();

        for json in [
            r#"{"@type":"updateNewChat","@client_id":11,"chat":{"id":1}}"#,
            r#"{"@type":"ok","@client_id":12}"#,
            r#"{"@type":"updateNewMessage","@client_id":11,"message":{"id":2}}"#,
        ] {
            registry.route_json(json.to_owned()).unwrap();
        }

        assert_eq!(
            first.try_iter().collect::<Vec<_>>(),
            vec![
                r#"{"@type":"updateNewChat","@client_id":11,"chat":{"id":1}}"#,
                r#"{"@type":"updateNewMessage","@client_id":11,"message":{"id":2}}"#,
            ]
        );
        assert_eq!(
            second.try_iter().collect::<Vec<_>>(),
            vec![r#"{"@type":"ok","@client_id":12}"#]
        );
    }

    #[test]
    fn rejects_messages_without_a_client_id_or_unknown_client() {
        let registry = ClientMailboxRegistry::default();

        assert_eq!(
            registry.route_json(r#"{"@type":"ok"}"#.to_owned()),
            Err(DispatchError::MissingClientId)
        );
        assert_eq!(
            registry.route_json(r#"{"@type":"ok","@client_id":19}"#.to_owned()),
            Err(DispatchError::UnknownClient(19))
        );
    }

    #[test]
    fn duplicate_registration_does_not_replace_the_original_mailbox() {
        let mut registry = ClientMailboxRegistry::default();
        let original = registry.register(23).unwrap();

        assert!(matches!(
            registry.register(23),
            Err(DispatchError::DuplicateClient(23))
        ));
        registry
            .route_json(r#"{"@type":"ok","@client_id":23}"#.to_owned())
            .unwrap();
        assert_eq!(
            original.try_recv().unwrap(),
            r#"{"@type":"ok","@client_id":23}"#
        );
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn reports_disconnected_mailboxes_and_supports_unregistering() {
        let mut registry = ClientMailboxRegistry::default();
        let receiver = registry.register(29).unwrap();
        drop(receiver);

        assert_eq!(
            registry.route_json(r#"{"@type":"ok","@client_id":29}"#.to_owned()),
            Err(DispatchError::MailboxDisconnected(29))
        );
        assert!(registry.unregister(29));
        assert!(!registry.unregister(29));
        assert!(registry.is_empty());
    }

    #[test]
    fn rejects_malformed_messages_before_routing() {
        let mut registry = ClientMailboxRegistry::default();
        let _receiver = registry.register(31).unwrap();

        assert!(matches!(
            registry.route_json(r#"{"@type":"ok","@client_id":31,}"#.to_owned()),
            Err(DispatchError::Decode(_))
        ));
    }
}


/// Thread-safe sender for requests addressed to a TDLib client ID.
///
/// The native implementation wraps td_send(client_id, request). It must keep
/// all FFI pointer conversion and native error handling inside that boundary.
pub trait ClientRequestSender: Send + Sync + 'static {
    /// Sends one JSON request to the selected TDLib client.
    fn send_request(&self, client_id: i32, request: &str) -> Result<(), String>;
}

/// A worker-facing transport backed by one account mailbox and client ID.
pub struct AccountMailboxTransport<S: ClientRequestSender> {
    client_id: i32,
    sender: Arc<S>,
    mailbox: Receiver<String>,
    close_requested: bool,
}

impl<S: ClientRequestSender> AccountMailboxTransport<S> {
    /// Connects a worker transport to its shared-dispatcher mailbox.
    pub fn new(client_id: i32, sender: Arc<S>, mailbox: Receiver<String>) -> Self {
        Self {
            client_id,
            sender,
            mailbox,
            close_requested: false,
        }
    }

    /// Returns the TDLib client ID used for outgoing requests.
    pub const fn client_id(&self) -> i32 {
        self.client_id
    }
}

impl<S: ClientRequestSender> JsonTransport for AccountMailboxTransport<S> {
    fn send(&mut self, request: &str) -> Result<(), TransportError> {
        if self.close_requested {
            return Err(TransportError::new(
                TransportOperation::Send,
                "account transport is closing",
            ));
        }
        self.sender
            .send_request(self.client_id, request)
            .map_err(|message| TransportError::new(TransportOperation::Send, message))
    }

    fn receive(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<String>, TransportError> {
        match self.mailbox.recv_timeout(timeout) {
            Ok(message) => Ok(Some(message)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(TransportError::new(
                TransportOperation::Receive,
                "account mailbox disconnected",
            )),
        }
    }

    fn close(&mut self) -> Result<(), TransportError> {
        if self.close_requested {
            return Ok(());
        }
        self.sender
            .send_request(self.client_id, r#"{"@type":"close"}"#)
            .map_err(|message| TransportError::new(TransportOperation::Close, message))?;
        self.close_requested = true;
        Ok(())
    }
}

/// Native receive source owned by the process-wide dispatcher thread.
///
/// A TDLib implementation must be the sole caller of the process-global
/// td_receive function. It must copy the returned C string into an owned
/// String before returning. Ok(None) represents a timeout, not closure.
pub trait SharedReceiveSource: Send + 'static {
    /// Receives the next message from TDLib's shared stream.
    fn receive(&mut self, timeout: Duration) -> Result<Option<String>, String>;
}

enum DispatcherCommand {
    Register {
        client_id: i32,
        reply: Sender<Result<Receiver<String>, DispatchError>>,
    },
    Unregister {
        client_id: i32,
        reply: Sender<bool>,
    },
    Stop,
}

/// Handle for the one process-wide TDLib receive dispatcher.
pub struct SharedDispatcher {
    commands: Sender<DispatcherCommand>,
    events: Receiver<DispatchError>,
    thread: Option<JoinHandle<()>>,
}

impl SharedDispatcher {
    /// Starts the only thread permitted to poll the native shared receive API.
    pub fn spawn<S: SharedReceiveSource>(source: S) -> std::io::Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let (event_sender, events) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("telestra-tdlib-dispatch".to_owned())
            .spawn(move || run_dispatcher(source, receiver, event_sender))?;
        Ok(Self {
            commands,
            events,
            thread: Some(thread),
        })
    }

    /// Registers an account and returns its dedicated ordered message mailbox.
    pub fn register_client(&self, client_id: i32) -> Result<Receiver<String>, DispatchError> {
        let (reply, response) = mpsc::channel();
        self.commands
            .send(DispatcherCommand::Register { client_id, reply })
            .map_err(|_| DispatchError::DispatcherStopped)?;
        response
            .recv()
            .map_err(|_| DispatchError::DispatcherStopped)?
    }

    /// Removes an account mailbox after its closed authorization state arrives.
    pub fn unregister_client(&self, client_id: i32) -> Result<bool, DispatchError> {
        let (reply, response) = mpsc::channel();
        self.commands
            .send(DispatcherCommand::Unregister { client_id, reply })
            .map_err(|_| DispatchError::DispatcherStopped)?;
        response
            .recv()
            .map_err(|_| DispatchError::DispatcherStopped)
    }

    /// Receives routing and native receive failures from the dispatcher.
    pub fn events(&self) -> &Receiver<DispatchError> {
        &self.events
    }

    /// Stops polling and joins the process-wide receive thread.
    pub fn shutdown(mut self) -> thread::Result<()> {
        let _ = self.commands.send(DispatcherCommand::Stop);
        if let Some(thread) = self.thread.take() {
            thread.join()
        } else {
            Ok(())
        }
    }
}

impl Drop for SharedDispatcher {
    fn drop(&mut self) {
        let _ = self.commands.send(DispatcherCommand::Stop);
    }
}

fn run_dispatcher<S: SharedReceiveSource>(
    mut source: S,
    commands: Receiver<DispatcherCommand>,
    events: Sender<DispatchError>,
) {
    const RECEIVE_TIMEOUT: Duration = Duration::from_millis(20);
    let mut registry = ClientMailboxRegistry::default();

    loop {
        match commands.recv_timeout(RECEIVE_TIMEOUT) {
            Ok(DispatcherCommand::Register { client_id, reply }) => {
                let _ = reply.send(registry.register(client_id));
            }
            Ok(DispatcherCommand::Unregister { client_id, reply }) => {
                let _ = reply.send(registry.unregister(client_id));
            }
            Ok(DispatcherCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        match source.receive(RECEIVE_TIMEOUT) {
            Ok(Some(json)) => {
                if let Err(error) = registry.route_json(json) {
                    let _ = events.send(error);
                }
            }
            Ok(None) => {}
            Err(message) => {
                let _ = events.send(DispatchError::ReceiveFailed(message));
                break;
            }
        }
    }
}

#[cfg(test)]
mod shared_dispatcher_tests {
    use super::{DispatchError, SharedDispatcher, SharedReceiveSource};
    use std::sync::mpsc;
    use std::time::Duration;


    #[derive(Default)]
    struct RecordingSender {
        sent: std::sync::Mutex<Vec<(i32, String)>>,
    }

    impl super::ClientRequestSender for RecordingSender {
        fn send_request(&self, client_id: i32, request: &str) -> Result<(), String> {
            self.sent
                .lock()
                .unwrap()
                .push((client_id, request.to_owned()));
            Ok(())
        }
    }

    #[test]
    fn account_transport_sends_with_client_id_and_receives_from_its_mailbox() {
        use super::AccountMailboxTransport;
        use crate::telegram::JsonTransport;
        use std::sync::Arc;

        let sender = Arc::new(RecordingSender::default());
        let (mailbox_sender, mailbox_receiver) = mpsc::channel();
        let mut transport = AccountMailboxTransport::new(53, Arc::clone(&sender), mailbox_receiver);

        transport.send(r#"{"@type":"getMe"}"#).unwrap();
        mailbox_sender
            .send(r#"{"@type":"user","@client_id":53}"#.to_owned())
            .unwrap();

        assert_eq!(transport.client_id(), 53);
        assert_eq!(
            transport.receive(Duration::from_secs(1)).unwrap(),
            Some(r#"{"@type":"user","@client_id":53}"#.to_owned())
        );
        assert_eq!(
            *sender.sent.lock().unwrap(),
            vec![(53, r#"{"@type":"getMe"}"#.to_owned())]
        );
    }

    #[test]
    fn account_transport_sends_close_once_and_rejects_later_requests() {
        use super::AccountMailboxTransport;
        use crate::telegram::{JsonTransport, TransportOperation};
        use std::sync::Arc;

        let sender = Arc::new(RecordingSender::default());
        let (_mailbox_sender, mailbox_receiver) = mpsc::channel();
        let mut transport = AccountMailboxTransport::new(59, Arc::clone(&sender), mailbox_receiver);

        transport.close().unwrap();
        transport.close().unwrap();
        let error = transport.send(r#"{"@type":"getMe"}"#).unwrap_err();

        assert_eq!(error.operation(), TransportOperation::Send);
        assert_eq!(
            *sender.sent.lock().unwrap(),
            vec![(59, r#"{"@type":"close"}"#.to_owned())]
        );
    }

    struct ChannelSource {
        incoming: mpsc::Receiver<String>,
    }

    impl SharedReceiveSource for ChannelSource {
        fn receive(&mut self, timeout: Duration) -> Result<Option<String>, String> {
            match self.incoming.recv_timeout(timeout) {
                Ok(message) => Ok(Some(message)),
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(mpsc::RecvTimeoutError::Disconnected) => Err("source disconnected".to_owned()),
            }
        }
    }

    #[test]
    fn shared_dispatcher_routes_messages_to_registered_account() {
        let (incoming, source) = mpsc::channel();
        let dispatcher = SharedDispatcher::spawn(ChannelSource { incoming }).unwrap();
        let account = dispatcher.register_client(41).unwrap();

        incoming
            .send(r#"{"@type":"updateNewChat","@client_id":41}"#.to_owned())
            .unwrap();

        assert_eq!(
            account.recv_timeout(Duration::from_secs(1)).unwrap(),
            r#"{"@type":"updateNewChat","@client_id":41}"#
        );
        assert!(dispatcher.unregister_client(41).unwrap());
        dispatcher.shutdown().unwrap();
    }

    #[test]
    fn shared_dispatcher_reports_messages_for_unregistered_clients() {
        let (incoming, source) = mpsc::channel();
        let dispatcher = SharedDispatcher::spawn(ChannelSource { incoming }).unwrap();

        incoming
            .send(r#"{"@type":"updateNewChat","@client_id":47}"#.to_owned())
            .unwrap();

        assert_eq!(
            dispatcher
                .events()
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            DispatchError::UnknownClient(47)
        );
        dispatcher.shutdown().unwrap();
    }

    #[test]
    fn shared_dispatcher_reports_native_receive_failures() {
        let (incoming, source) = mpsc::channel();
        let dispatcher = SharedDispatcher::spawn(ChannelSource { incoming }).unwrap();
        drop(source);

        assert_eq!(
            dispatcher.events().recv_timeout(Duration::from_secs(1)).unwrap(),
            DispatchError::ReceiveFailed("source disconnected".to_owned())
        );
        dispatcher.shutdown().unwrap();
    }

    #[test]
    fn shared_dispatcher_rejects_duplicate_client_registration() {
        let (_incoming, source) = mpsc::channel();
        let dispatcher = SharedDispatcher::spawn(ChannelSource { incoming: source }).unwrap();
        let _first = dispatcher.register_client(43).unwrap();

        assert!(matches!(
            dispatcher.register_client(43),
            Err(DispatchError::DuplicateClient(43))
        ));
        dispatcher.shutdown().unwrap();
    }
}
