# Transfer queue feasibility (TDLib 1.8.69)

This note records the API boundary for issue #15 against the TDLib 1.8.69 source revision pinned in `packaging/tdlib/source.env`. It is an implementation plan, not a claim that a transfer UI or runtime queue already exists.

## What TDLib supports

### Message-attachment download list

TDLib exposes a first-class download list for files attached to messages:

- `addFileToDownloads(file_id, chat_id, message_id, priority)` adds a message attachment. The list is persistent across application restarts when TDLib's message database is enabled. This list is independent of a plain `downloadFile` request.
- `searchFileDownloads(query, only_active, only_completed, offset, limit)` can enumerate the list. `only_active=true` includes paused downloads.
- `toggleDownloadIsPaused(file_id, is_paused)` pauses or resumes one item in this list.
- `toggleAllDownloadsArePaused(are_paused)` pauses or resumes all items in this list.
- `updateFileDownload` reports list-item changes and its paused/completed state. `updateFile` reports file-level transfer state and progress.

These APIs support genuine per-item and global pause/resume for items managed by TDLib's message-attachment download list.

## Direct file downloads are a different path

`downloadFile(file_id, priority, offset, limit, synchronous)` starts a direct download. Its priority is 1–32 (higher is earlier); offset/limit can request a byte range; and `synchronous=false` returns the current file state without waiting for completion. `cancelDownloadFile(file_id, only_if_pending)` stops a direct download.

The pause APIs above are explicitly scoped to the file download list. Do not present them as a universal pause mechanism for every arbitrary `downloadFile` transfer. A future direct-download resume flow must be designed and tested around the file's actual local/remote state and safe offsets; do not infer resumability merely from the presence of offset/limit parameters.

## Proposed product behavior

1. Use TDLib's download list for message attachments when queue persistence and pause/resume controls are requested.
2. Show item progress from `updateFile`; show paused/completed/list membership from `updateFileDownload` and the initial `searchFileDownloads` result.
3. Expose per-item pause/resume and global pause/resume only for list-managed downloads.
4. Keep cancel distinct from pause: use `cancelDownloadFile` where appropriate and do not silently remove a list item unless the user explicitly asks to remove it.
5. Keep queue state owned by the backend model; GTK only renders it and sends user intents.
6. Treat upload controls as out of scope for this issue until TDLib's upload lifecycle is separately investigated.

## Limits and edge cases

- The TDLib download list contains message attachments; it is not a generic filesystem download manager.
- List persistence depends on TDLib's message database being enabled.
- A direct `downloadFile` request is not automatically a list item and must not be assumed to respond to list pause commands.
- Priority is bounded to 1–32. Equal-priority ordering is TDLib-defined (most recently requested first), not arbitrary drag-and-drop order.
- The initial download list must be loaded before relying on subsequent `updateFileDownload` notifications; TDLib documents that these updates are sent only after the list is first loaded.
- One file should not be scheduled as multiple concurrent ranges; TDLib maintainers state that only one concurrent download request per file is supported.

## Verification plan

- Unit-test queue state transitions with fake TDLib events: queued, active, paused, resumed, completed, canceled, and removed.
- Test global pause/resume without changing each item's identity or progress.
- Test pagination and initial list hydration before applying incremental updates.
- Test direct-download cancellation separately from list pause/resume.
- Verify no UI work occurs on the TDLib receive thread.

## Primary references

- [addFileToDownloads](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1add_file_to_downloads.html)
- [searchFileDownloads](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1search_file_downloads.html)
- [toggleDownloadIsPaused](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1toggle_download_is_paused.html)
- [toggleAllDownloadsArePaused](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1toggle_all_downloads_are_paused.html)
- [downloadFile](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1download_file.html)
- [cancelDownloadFile](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1cancel_download_file.html)
- [updateFileDownload](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1update_file_download.html)
- [TDLib maintainer clarification on concurrent ranges](https://github.com/tdlib/td/issues/3017)
