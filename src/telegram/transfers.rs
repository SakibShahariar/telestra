//! Application-owned state for TDLib's message-attachment download list.
//!
//! TDLib owns the actual transfer engine. This module tracks the user-visible
//! queue and emits typed intents; callers translate those intents into TDLib
//! JSON requests and apply authoritative `updateFile`/`updateFileDownload`
//! events back into this model.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use super::protocol::RequestId;

/// Current state of one message-attachment download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed(String),
}

/// File state received from TDLib's `updateFile` object.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileTransferState {
    pub is_downloading_active: bool,
    pub is_downloading_completed: bool,
    pub download_offset: u64,
    pub downloaded_prefix_size: u64,
    pub downloaded_size: u64,
    pub expected_size: u64,
}

/// Progress values retained for display and safe recovery decisions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransferProgress {
    /// Approximate total downloaded bytes; display only, not a resume offset.
    pub downloaded_size: u64,
    /// Expected total file size, or zero when unknown.
    pub expected_size: u64,
    /// TDLib offset from which the contiguous downloaded prefix is measured.
    pub download_offset: u64,
    /// Contiguous downloaded prefix beginning at `download_offset`.
    pub downloaded_prefix_size: u64,
}

/// One item in TDLib's persistent message-attachment download list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferItem {
    file_id: i32,
    chat_id: i64,
    message_id: i64,
    priority: Option<u8>,
    status: TransferStatus,
    progress: TransferProgress,
}

impl TransferItem {
    pub const fn file_id(&self) -> i32 {
        self.file_id
    }

    pub const fn chat_id(&self) -> i64 {
        self.chat_id
    }

    pub const fn message_id(&self) -> i64 {
        self.message_id
    }

    pub const fn priority(&self) -> Option<u8> {
        self.priority
    }

    pub fn status(&self) -> &TransferStatus {
        &self.status
    }

    pub const fn progress(&self) -> TransferProgress {
        self.progress
    }
}

/// Typed TDLib operations requested by the queue model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferCommand {
    AddToDownloads {
        file_id: i32,
        chat_id: i64,
        message_id: i64,
        priority: u8,
    },
    TogglePaused {
        file_id: i32,
        is_paused: bool,
    },
    ToggleAllPaused {
        are_paused: bool,
    },
    RemoveFromDownloads {
        file_id: i32,
        /// Always false for the normal remove action; cache deletion requires
        /// a separate explicit user choice.
        delete_from_cache: bool,
    },
}

impl TransferCommand {
    /// Returns the TDLib operation name used for worker diagnostics.
    pub const fn operation_name(&self) -> &'static str {
        match self {
            Self::AddToDownloads { .. } => "addFileToDownloads",
            Self::TogglePaused { .. } => "toggleDownloadIsPaused",
            Self::ToggleAllPaused { .. } => "toggleAllDownloadsArePaused",
            Self::RemoveFromDownloads { .. } => "removeFileFromDownloads",
        }
    }

    /// Serializes this intent as a TDLib JSON request with worker correlation.
    ///
    /// All fields are numeric or boolean, and the request ID's extra value is
    /// generated internally, so no caller-provided text is interpolated.
    pub fn to_tdlib_json(&self, request_id: RequestId) -> String {
        let extra = request_id.to_extra();
        match self {
            Self::AddToDownloads {
                file_id,
                chat_id,
                message_id,
                priority,
            } => format!(
                r#"{{"@type":"addFileToDownloads","file_id":{file_id},"chat_id":{chat_id},"message_id":{message_id},"priority":{priority},"@extra":"{extra}"}}"#
            ),
            Self::TogglePaused { file_id, is_paused } => format!(
                r#"{{"@type":"toggleDownloadIsPaused","file_id":{file_id},"is_paused":{is_paused},"@extra":"{extra}"}}"#
            ),
            Self::ToggleAllPaused { are_paused } => format!(
                r#"{{"@type":"toggleAllDownloadsArePaused","are_paused":{are_paused},"@extra":"{extra}"}}"#
            ),
            Self::RemoveFromDownloads {
                file_id,
                delete_from_cache,
            } => format!(
                r#"{{"@type":"removeFileFromDownloads","file_id":{file_id},"delete_from_cache":{delete_from_cache},"@extra":"{extra}"}}"#
            ),
        }
    }
}

/// Errors returned when a transfer intent cannot be represented safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferQueueError {
    InvalidFileId(i32),
    InvalidPriority(u8),
    DuplicateFileId(i32),
    UnknownFileId(i32),
    NotPausable(i32),
}

impl fmt::Display for TransferQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFileId(file_id) => write!(formatter, "invalid TDLib file ID: {file_id}"),
            Self::InvalidPriority(priority) => {
                write!(
                    formatter,
                    "TDLib download priority must be 1–32, got {priority}"
                )
            }
            Self::DuplicateFileId(file_id) => {
                write!(formatter, "file {file_id} is already in the transfer queue")
            }
            Self::UnknownFileId(file_id) => {
                write!(formatter, "file {file_id} is not in the transfer queue")
            }
            Self::NotPausable(file_id) => {
                write!(formatter, "file {file_id} is already completed or failed")
            }
        }
    }
}

impl Error for TransferQueueError {}

/// Ordered application view of TDLib's message-attachment download list.
#[derive(Debug, Default)]
pub struct TransferQueue {
    items: BTreeMap<i32, TransferItem>,
    insertion_order: Vec<i32>,
}

impl TransferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a message attachment to the local view and returns the TDLib intent.
    ///
    /// The returned command must be sent to TDLib. The queue item remains
    /// provisional until the corresponding TDLib update confirms list state.
    pub fn enqueue_message_attachment(
        &mut self,
        file_id: i32,
        chat_id: i64,
        message_id: i64,
        priority: u8,
    ) -> Result<TransferCommand, TransferQueueError> {
        if file_id <= 0 {
            return Err(TransferQueueError::InvalidFileId(file_id));
        }
        if !(1..=32).contains(&priority) {
            return Err(TransferQueueError::InvalidPriority(priority));
        }
        if self.items.contains_key(&file_id) {
            return Err(TransferQueueError::DuplicateFileId(file_id));
        }

        self.items.insert(
            file_id,
            TransferItem {
                file_id,
                chat_id,
                message_id,
                priority: Some(priority),
                status: TransferStatus::Queued,
                progress: TransferProgress::default(),
            },
        );
        self.insertion_order.push(file_id);

        Ok(TransferCommand::AddToDownloads {
            file_id,
            chat_id,
            message_id,
            priority,
        })
    }

    /// Restores an item returned by TDLib's initial `searchFileDownloads` query.
    ///
    /// TDLib's `fileDownload` object does not include its original priority,
    /// so restored items deliberately expose `None` rather than inventing one.
    pub fn restore_message_attachment(
        &mut self,
        file_id: i32,
        chat_id: i64,
        message_id: i64,
        complete_date: i32,
        is_paused: bool,
    ) -> Result<(), TransferQueueError> {
        if file_id <= 0 {
            return Err(TransferQueueError::InvalidFileId(file_id));
        }
        if self.items.contains_key(&file_id) {
            return Err(TransferQueueError::DuplicateFileId(file_id));
        }

        let status = if complete_date > 0 {
            TransferStatus::Completed
        } else if is_paused {
            TransferStatus::Paused
        } else {
            TransferStatus::Queued
        };
        self.items.insert(
            file_id,
            TransferItem {
                file_id,
                chat_id,
                message_id,
                priority: None,
                status,
                progress: TransferProgress::default(),
            },
        );
        self.insertion_order.push(file_id);
        Ok(())
    }

    /// Requests pause or resume for one list-managed item.
    ///
    /// TDLib updates remain authoritative; this method does not optimistically
    /// change the displayed status.
    pub fn set_item_paused(
        &self,
        file_id: i32,
        is_paused: bool,
    ) -> Result<Option<TransferCommand>, TransferQueueError> {
        let item = self
            .items
            .get(&file_id)
            .ok_or(TransferQueueError::UnknownFileId(file_id))?;

        match &item.status {
            TransferStatus::Completed | TransferStatus::Failed(_) => {
                return Err(TransferQueueError::NotPausable(file_id));
            }
            TransferStatus::Paused if is_paused => return Ok(None),
            TransferStatus::Queued | TransferStatus::Downloading if !is_paused => {
                return Ok(None);
            }
            TransferStatus::Queued | TransferStatus::Downloading | TransferStatus::Paused => {}
        }

        Ok(Some(TransferCommand::TogglePaused { file_id, is_paused }))
    }

    /// Requests a global pause/resume for the TDLib download list.
    pub fn set_all_paused(&self, are_paused: bool) -> Option<TransferCommand> {
        let needs_change = self.items.values().any(|item| match &item.status {
            TransferStatus::Queued | TransferStatus::Downloading => are_paused,
            TransferStatus::Paused => !are_paused,
            TransferStatus::Completed | TransferStatus::Failed(_) => false,
        });

        needs_change.then_some(TransferCommand::ToggleAllPaused { are_paused })
    }

    /// Requests removal from TDLib's list without deleting the cached file.
    ///
    /// The item is removed from this model only after TDLib sends the matching
    /// `updateFileRemovedFromDownloads` event.
    pub fn request_remove(&self, file_id: i32) -> Result<TransferCommand, TransferQueueError> {
        if !self.items.contains_key(&file_id) {
            return Err(TransferQueueError::UnknownFileId(file_id));
        }
        Ok(TransferCommand::RemoveFromDownloads {
            file_id,
            delete_from_cache: false,
        })
    }

    /// Applies TDLib's file-download-list update.
    pub fn apply_download_list_update(
        &mut self,
        file_id: i32,
        complete_date: i32,
        is_paused: bool,
    ) -> bool {
        let Some(item) = self.items.get_mut(&file_id) else {
            return false;
        };

        item.status = if complete_date > 0 {
            TransferStatus::Completed
        } else if is_paused {
            TransferStatus::Paused
        } else if item.status == TransferStatus::Downloading {
            TransferStatus::Downloading
        } else {
            TransferStatus::Queued
        };
        true
    }

    /// Applies the authoritative local-file state from TDLib.
    pub fn apply_file_state(&mut self, file_id: i32, state: FileTransferState) -> bool {
        let Some(item) = self.items.get_mut(&file_id) else {
            return false;
        };

        item.progress = TransferProgress {
            downloaded_size: state.downloaded_size,
            expected_size: state.expected_size,
            download_offset: state.download_offset,
            downloaded_prefix_size: state.downloaded_prefix_size,
        };
        item.status = if state.is_downloading_completed {
            TransferStatus::Completed
        } else if state.is_downloading_active {
            TransferStatus::Downloading
        } else if item.status == TransferStatus::Paused {
            TransferStatus::Paused
        } else {
            TransferStatus::Queued
        };
        true
    }

    /// Records a terminal error without losing the item or its last progress.
    pub fn mark_failed(&mut self, file_id: i32, reason: impl Into<String>) -> bool {
        let Some(item) = self.items.get_mut(&file_id) else {
            return false;
        };
        item.status = TransferStatus::Failed(reason.into());
        true
    }

    /// Applies TDLib's updateFileRemovedFromDownloads event.
    pub fn apply_removed(&mut self, file_id: i32) -> Option<TransferItem> {
        let item = self.items.remove(&file_id)?;
        self.insertion_order
            .retain(|queued_id| *queued_id != file_id);
        Some(item)
    }

    pub fn get(&self, file_id: i32) -> Option<&TransferItem> {
        self.items.get(&file_id)
    }

    /// Iterates in the order items were added to the local queue.
    pub fn items(&self) -> impl Iterator<Item = &TransferItem> {
        self.insertion_order
            .iter()
            .filter_map(|file_id| self.items.get(file_id))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FileTransferState, TransferCommand, TransferProgress, TransferQueue, TransferQueueError,
        TransferStatus,
    };

    fn queue_with_one_item() -> TransferQueue {
        let mut queue = TransferQueue::new();
        queue.enqueue_message_attachment(42, -100, 7, 16).unwrap();
        queue
    }

    #[test]
    fn transfer_commands_serialize_as_correlated_tdlib_requests() {
        use super::TransferCommand;
        use crate::telegram::RequestId;

        let request_id = RequestId::new(7);
        let cases = [
            (
                TransferCommand::AddToDownloads {
                    file_id: 42,
                    chat_id: -100,
                    message_id: 9,
                    priority: 16,
                },
                r#"{"@type":"addFileToDownloads","file_id":42,"chat_id":-100,"message_id":9,"priority":16,"@extra":"telestra:7"}"#,
            ),
            (
                TransferCommand::TogglePaused {
                    file_id: 42,
                    is_paused: true,
                },
                r#"{"@type":"toggleDownloadIsPaused","file_id":42,"is_paused":true,"@extra":"telestra:7"}"#,
            ),
            (
                TransferCommand::ToggleAllPaused { are_paused: false },
                r#"{"@type":"toggleAllDownloadsArePaused","are_paused":false,"@extra":"telestra:7"}"#,
            ),
            (
                TransferCommand::RemoveFromDownloads {
                    file_id: 42,
                    delete_from_cache: false,
                },
                r#"{"@type":"removeFileFromDownloads","file_id":42,"delete_from_cache":false,"@extra":"telestra:7"}"#,
            ),
        ];

        for (command, expected) in cases {
            assert_eq!(command.to_tdlib_json(request_id), expected);
        }
    }

    #[test]
    fn applies_decoded_tdlib_updates_to_existing_queue_items() {
        use super::TransferUpdate;

        let mut queue = queue_with_one_item();
        assert!(
            queue.apply_tdlib_update(TransferUpdate::DownloadListChanged {
                file_id: 42,
                complete_date: 0,
                is_paused: true,
            })
        );
        assert_eq!(queue.get(42).unwrap().status(), &TransferStatus::Paused);

        assert!(queue.apply_tdlib_update(TransferUpdate::FileStateChanged {
            file_id: 42,
            state: FileTransferState {
                is_downloading_active: true,
                is_downloading_completed: false,
                download_offset: 0,
                downloaded_prefix_size: 2048,
                downloaded_size: 2048,
                expected_size: 8192,
            },
        }));
        assert_eq!(
            queue.get(42).unwrap().status(),
            &TransferStatus::Downloading
        );
        assert_eq!(
            queue.get(42).unwrap().progress().downloaded_prefix_size,
            2048
        );

        assert!(queue.apply_tdlib_update(TransferUpdate::RemovedFromDownloads { file_id: 42 }));
        assert!(queue.is_empty());
        assert!(!queue.apply_tdlib_update(TransferUpdate::RemovedFromDownloads { file_id: 42 }));
    }

    #[test]
    fn enqueues_message_attachments_with_tdlib_priority() {
        let mut queue = TransferQueue::new();
        let command = queue.enqueue_message_attachment(42, -100, 7, 16).unwrap();

        assert_eq!(
            command,
            TransferCommand::AddToDownloads {
                file_id: 42,
                chat_id: -100,
                message_id: 7,
                priority: 16,
            }
        );
        let item = queue.get(42).unwrap();
        assert_eq!(item.status(), &TransferStatus::Queued);
        assert_eq!(item.priority(), Some(16));
        assert_eq!(item.progress(), TransferProgress::default());
    }

    #[test]
    fn rejects_duplicate_file_ids_and_invalid_priorities() {
        let mut queue = queue_with_one_item();
        assert_eq!(
            queue.enqueue_message_attachment(42, -100, 7, 1),
            Err(TransferQueueError::DuplicateFileId(42))
        );
        assert_eq!(
            queue.enqueue_message_attachment(43, -100, 8, 0),
            Err(TransferQueueError::InvalidPriority(0))
        );
        assert_eq!(
            queue.enqueue_message_attachment(44, -100, 9, 33),
            Err(TransferQueueError::InvalidPriority(33))
        );
        assert_eq!(
            queue.enqueue_message_attachment(0, -100, 9, 1),
            Err(TransferQueueError::InvalidFileId(0))
        );
    }

    #[test]
    fn restores_persistent_items_without_inventing_priority() {
        let mut queue = TransferQueue::new();
        queue
            .restore_message_attachment(55, -100, 88, 0, true)
            .unwrap();

        let item = queue.get(55).unwrap();
        assert_eq!(item.priority(), None);
        assert_eq!(item.status(), &TransferStatus::Paused);
        assert_eq!(item.chat_id(), -100);
        assert_eq!(item.message_id(), 88);
    }

    #[test]
    fn pause_and_resume_are_explicit_tdlib_commands() {
        let mut queue = queue_with_one_item();
        assert_eq!(
            queue.set_item_paused(42, true).unwrap(),
            Some(TransferCommand::TogglePaused {
                file_id: 42,
                is_paused: true,
            })
        );
        queue.apply_download_list_update(42, 0, true);
        assert_eq!(queue.get(42).unwrap().status(), &TransferStatus::Paused);
        assert_eq!(queue.set_item_paused(42, true).unwrap(), None);
        assert_eq!(
            queue.set_item_paused(42, false).unwrap(),
            Some(TransferCommand::TogglePaused {
                file_id: 42,
                is_paused: false,
            })
        );
    }

    #[test]
    fn global_pause_and_resume_skip_noop_requests() {
        let mut queue = queue_with_one_item();
        assert_eq!(
            queue.set_all_paused(true),
            Some(TransferCommand::ToggleAllPaused { are_paused: true })
        );
        queue.apply_download_list_update(42, 0, true);
        assert_eq!(queue.set_all_paused(true), None);
        assert_eq!(
            queue.set_all_paused(false),
            Some(TransferCommand::ToggleAllPaused { are_paused: false })
        );
    }

    #[test]
    fn file_updates_preserve_safe_prefix_separately_from_approximate_progress() {
        let mut queue = queue_with_one_item();
        assert!(queue.apply_file_state(
            42,
            FileTransferState {
                is_downloading_active: true,
                is_downloading_completed: false,
                download_offset: 1024,
                downloaded_prefix_size: 4096,
                downloaded_size: 5120,
                expected_size: 16384,
            },
        ));

        let item = queue.get(42).unwrap();
        assert_eq!(item.status(), &TransferStatus::Downloading);
        assert_eq!(
            item.progress(),
            TransferProgress {
                downloaded_size: 5120,
                expected_size: 16384,
                download_offset: 1024,
                downloaded_prefix_size: 4096,
            }
        );
    }

    #[test]
    fn completion_and_failure_prevent_pause_requests() {
        let mut queue = queue_with_one_item();
        queue.apply_download_list_update(42, 123, false);
        assert_eq!(queue.get(42).unwrap().status(), &TransferStatus::Completed);
        assert_eq!(
            queue.set_item_paused(42, true),
            Err(TransferQueueError::NotPausable(42))
        );

        queue.apply_file_state(
            42,
            FileTransferState {
                is_downloading_active: false,
                is_downloading_completed: false,
                ..FileTransferState::default()
            },
        );
        queue.mark_failed(42, "network failure");
        assert_eq!(
            queue.set_item_paused(42, false),
            Err(TransferQueueError::NotPausable(42))
        );
    }

    #[test]
    fn remove_is_confirmed_by_tdlib_and_does_not_delete_cache() {
        let mut queue = queue_with_one_item();
        assert_eq!(
            queue.request_remove(42).unwrap(),
            TransferCommand::RemoveFromDownloads {
                file_id: 42,
                delete_from_cache: false,
            }
        );
        assert_eq!(queue.len(), 1);
        assert!(queue.apply_removed(42).is_some());
        assert!(queue.is_empty());
    }

    #[test]
    fn unknown_events_do_not_create_queue_items() {
        let mut queue = TransferQueue::new();
        assert!(!queue.apply_download_list_update(99, 0, true));
        assert!(!queue.apply_file_state(99, FileTransferState::default()));
        assert!(!queue.mark_failed(99, "unknown"));
        assert!(queue.apply_removed(99).is_none());
        assert!(queue.is_empty());
    }

    #[test]
    fn queue_iteration_preserves_insertion_order() {
        let mut queue = TransferQueue::new();
        queue.enqueue_message_attachment(9, 1, 1, 1).unwrap();
        queue.enqueue_message_attachment(3, 1, 2, 2).unwrap();
        queue.enqueue_message_attachment(7, 1, 3, 3).unwrap();
        assert_eq!(
            queue.items().map(|item| item.file_id()).collect::<Vec<_>>(),
            vec![9, 3, 7]
        );
    }
}
