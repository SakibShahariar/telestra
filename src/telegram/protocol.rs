//! Transport-independent pieces of the TDLib JSON request protocol.
//!
//! This module deliberately does not parse TDLib JSON or own a native client.
//! It provides deterministic request identifiers and pending-request tracking
//! that can be tested without linking TDLib or using a Telegram account.

use std::collections::BTreeMap;

/// Identifies one application request awaiting a TDLib response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(u64);

impl RequestId {
    /// Creates an identifier from a monotonically increasing counter.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the string value to place in a TDLib request's @extra field.
    pub fn to_extra(self) -> String {
        format!("telestra:{}", self.0)
    }

    /// Parses an @extra string emitted by this type.
    pub fn from_extra(value: &str) -> Option<Self> {
        value.strip_prefix("telestra:")?.parse().ok().map(Self)
    }
}

/// Generates unique request identifiers without wrapping on exhaustion.
///
/// Keep one sequence per TDLib client. A fresh sequence starts at 1; if the
/// integer space is exhausted, generation stops rather than reusing an ID
/// that could still identify an in-flight request.
#[derive(Debug, Clone)]
pub struct RequestIdSequence {
    next: Option<u64>,
}

impl Default for RequestIdSequence {
    fn default() -> Self {
        Self { next: Some(1) }
    }
}

impl RequestIdSequence {
    /// Creates a sequence whose first generated identifier has the given value.
    pub const fn starting_at(first: u64) -> Self {
        Self { next: Some(first) }
    }

    /// Returns the next unique identifier, or None after integer exhaustion.
    pub fn next_id(&mut self) -> Option<RequestId> {
        let value = self.next?;
        self.next = value.checked_add(1);
        Some(RequestId::new(value))
    }
}

/// Tracks application operations that are waiting for TDLib responses.
///
/// The owner should insert an operation before sending its request and resolve
/// it only when a response with the matching @extra arrives. Unmatched
/// updates and responses must not remove unrelated pending operations.
#[derive(Debug, Default)]
pub struct PendingRequests<T> {
    entries: BTreeMap<RequestId, T>,
}

impl<T> PendingRequests<T> {
    /// Creates an empty pending-request collection.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Registers an operation, returning any operation previously using the ID.
    pub fn insert(&mut self, id: RequestId, operation: T) -> Option<T> {
        self.entries.insert(id, operation)
    }

    /// Removes and returns the operation associated with a response ID.
    pub fn resolve(&mut self, id: RequestId) -> Option<T> {
        self.entries.remove(&id)
    }

    /// Resolves a response using the exact `@extra` value emitted by Telestra.
    ///
    /// Malformed, foreign or unknown values leave all pending operations intact.
    pub fn resolve_extra(&mut self, extra: &str) -> Option<T> {
        RequestId::from_extra(extra).and_then(|id| self.resolve(id))
    }

    /// Returns whether an operation is waiting for the given response.
    pub fn contains(&self, id: RequestId) -> bool {
        self.entries.contains_key(&id)
    }

    /// Returns the number of operations waiting for responses.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether no operations are waiting for responses.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{PendingRequests, RequestId, RequestIdSequence};

    #[test]
    fn request_id_round_trips_through_extra() {
        let id = RequestId::new(42);
        assert_eq!(RequestId::from_extra(&id.to_extra()), Some(id));
    }

    #[test]
    fn request_sequence_generates_distinct_ids() {
        let mut sequence = RequestIdSequence::default();
        let first = sequence.next_id().unwrap();
        let second = sequence.next_id().unwrap();
        assert_ne!(first, second);
        assert_eq!(first, RequestId::new(1));
        assert_eq!(second, RequestId::new(2));
    }

    #[test]
    fn request_sequence_stops_instead_of_wrapping() {
        let mut sequence = RequestIdSequence::starting_at(u64::MAX);
        assert_eq!(sequence.next_id(), Some(RequestId::new(u64::MAX)));
        assert_eq!(sequence.next_id(), None);
        assert_eq!(sequence.next_id(), None);
    }

    #[test]
    fn malformed_or_foreign_extra_values_are_rejected() {
        for value in ["42", "telestra:", "telestra:-1", "other:42"] {
            assert_eq!(RequestId::from_extra(value), None);
        }
    }

    #[test]
    fn resolving_one_response_leaves_other_requests_pending() {
        let first = RequestId::new(1);
        let second = RequestId::new(2);
        let mut pending = PendingRequests::new();
        pending.insert(first, "first operation");
        pending.insert(second, "second operation");

        assert_eq!(pending.resolve(second), Some("second operation"));
        assert!(pending.contains(first));
        assert!(!pending.contains(second));
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn unknown_response_does_not_consume_pending_operations() {
        let known = RequestId::new(7);
        let mut pending = PendingRequests::new();
        pending.insert(known, "operation");

        assert_eq!(pending.resolve(RequestId::new(99)), None);
        assert!(pending.contains(known));
    }

    #[test]
    fn response_extra_resolves_only_its_matching_operation() {
        let known = RequestId::new(8);
        let mut pending = PendingRequests::new();
        pending.insert(known, "operation");

        assert_eq!(pending.resolve_extra(&known.to_extra()), Some("operation"));
        assert!(pending.is_empty());
    }

    #[test]
    fn malformed_or_foreign_extra_does_not_consume_pending_operations() {
        let known = RequestId::new(9);
        let mut pending = PendingRequests::new();
        pending.insert(known, "operation");

        assert_eq!(pending.resolve_extra("foreign:9"), None);
        assert_eq!(pending.resolve_extra("telestra:not-a-number"), None);
        assert!(pending.contains(known));
    }

    #[test]
    fn duplicate_id_replacement_is_reported() {
        let id = RequestId::new(3);
        let mut pending = PendingRequests::new();
        assert_eq!(pending.insert(id, "old"), None);
        assert_eq!(pending.insert(id, "new"), Some("old"));
        assert_eq!(pending.resolve(id), Some("new"));
        assert!(pending.is_empty());
    }
}
