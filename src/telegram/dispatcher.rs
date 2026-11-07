//! Per-account mailboxes for TDLib's process-wide JSON receive stream.
//!
//! The native dispatcher owns this registry and is the only component that
//! calls TDLib's global td_receive. It routes each owned JSON message by the
//! @client_id field before account workers consume their mailboxes.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender};

use super::{DecodeError, decode_envelope};

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
}

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingClientId => formatter.write_str("TDLib message has no @client_id"),
            Self::UnknownClient(client_id) => {
                write!(formatter, "no mailbox registered for TDLib client {client_id}")
            }
            Self::DuplicateClient(client_id) => {
                write!(formatter, "TDLib client {client_id} already has a mailbox")
            }
            Self::MailboxDisconnected(client_id) => {
                write!(formatter, "mailbox for TDLib client {client_id} is disconnected")
            }
            Self::Decode(error) => write!(formatter, "invalid TDLib message: {error}"),
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
        if self.mailboxes.contains_key(&client_id) {
            return Err(DispatchError::DuplicateClient(client_id));
        }

        let (sender, receiver) = mpsc::channel();
        self.mailboxes.insert(client_id, sender);
        Ok(receiver)
    }

    /// Routes an owned TDLib JSON message using its @client_id field.
    ///
    /// The original JSON is forwarded unchanged so the account worker can
    /// decode and process it without losing fields unknown to this version.
    pub fn route_json(&self, json: String) -> Result<(), DispatchError> {
        let envelope = decode_envelope(&json).map_err(DispatchError::Decode)?;
        let client_id = envelope
            .client_id
            .ok_or(DispatchError::MissingClientId)?;
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
