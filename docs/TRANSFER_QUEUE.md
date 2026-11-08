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

## Current Rust adapter implementation

The application-owned queue model now has a typed transfer-intent boundary:

- `TransferCommand` serializes add, per-item pause/resume, global pause/resume and non-destructive remove operations into TDLib JSON requests carrying the worker's canonical `@extra` correlation ID.
- `BackendWorker::send_transfer_command` sends those commands through the normal pending-request registration and response-correlation path.
- `decode_transfer_update` recognizes `updateFileAddedToDownloads`, `updateFileDownload`, `updateFile` and `updateFileRemovedFromDownloads`; malformed required fields are rejected, unrelated update types remain available to other handlers. The added-to-list event restores message/chat identity without inventing a priority.
- `TransferQueue::apply_tdlib_update` applies only authoritative TDLib updates to existing queue items. It does not create phantom items from progress events, and removal is applied only after TDLib confirms it.
- File progress retains approximate downloaded size separately from the verified contiguous prefix and its offset.

The backend now decodes `foundFileDownloads` pages, retains TDLib's opaque `next_offset` cursor and aggregate counts, builds correlated `searchFileDownloads` requests, and can restore each page into the queue model without inventing a priority or duplicating entries. The account startup sequence still does not issue the initial query, walk subsequent pages, or coordinate live updates with hydration. GTK queue presentation is also not connected. This remains a backend model/adapter implementation, not a finished user-facing transfer manager.

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
- The initial download list must be loaded before relying on subsequent `updateFileDownload` notifications; TDLib documents that these updates are sent only after the list is first loaded. Use the returned opaque `next_offset` unchanged until it is empty, and reconcile incremental add/remove events that arrive while pages are being hydrated.
- One file should not be scheduled as multiple concurrent ranges; TDLib maintainers state that only one concurrent download request per file is supported.

## Concurrency limits (issue #16)

TDLib and Telegram already define parallel-download limits per data center. Telegram's file API documentation says clients should limit parallel downloads from the same DC to the configured `small_queue_active_operations_max` for files smaller than 20 MB and `large_queue_active_operations_max` for files larger than 20 MB. These values are supplied as client configuration; they are not a license to exceed server limits.

Implementation direction:

- Let TDLib own the actual transfer scheduling wherever possible; do not create parallel range requests for the same file.
- If Telestra adds an app-level concurrency preference, treat it as a lower client-side ceiling and clamp it to the current per-DC small/large limits.
- Count active transfers per data center and size class, not just one global number.
- Re-evaluate limits when TDLib configuration changes; never assume a fixed number is safe across accounts, DCs or versions.
- Keep upload concurrency separate. The cited download limits do not establish upload limits.

References:
- [Telegram file transfer limits](https://core.telegram.org/api/files)
- [TDLib autoDownloadSettings](https://core.telegram.org/constructor/autoDownloadSettings)
- [TDLib options and configuration](https://core.telegram.org/tdlib/options)

## Interrupted transfers and recovery (issue #17)

TDLib exposes enough local-file state to distinguish a real contiguous downloaded prefix from approximate transfer progress:

- `localFile.is_downloading_active` indicates whether the download is still active.
- `localFile.is_downloading_completed` indicates whether the local copy is complete.
- `localFile.download_offset` is the offset from which the downloaded prefix is measured.
- `localFile.downloaded_prefix_size` is the contiguous prefix available from that offset when the file is incomplete.
- `localFile.downloaded_size` is only suitable for approximate progress. TDLib explicitly warns that the actual file may be larger and some downloaded parts may contain garbage; never use this field alone as a resume offset.
- `getFileDownloadedPrefixSize(file_id, offset)` can query the downloaded prefix from a chosen offset.
- `downloadFile(file_id, priority, offset, limit, synchronous=false)` accepts a starting offset and range limit; progress/completion arrives through `updateFile`.

Recovery policy:

1. Prefer TDLib's own persisted file state and normal download lifecycle; do not manually splice or rename partial files.
2. On restart or reconnect, fetch the current `File` state and wait until `is_downloading_active=false` before deciding whether the last attempt stopped or completed.
3. If incomplete, use the TDLib-reported `download_offset` and contiguous `downloaded_prefix_size` (or `getFileDownloadedPrefixSize`) to reason about the verified prefix. Never resume from approximate `downloaded_size`.
4. Retry using TDLib's file ID and its documented download API; classify permanent errors separately from transient/network failures and apply bounded backoff.
5. Never run concurrent range requests for the same file. TDLib maintainers explicitly state that only one concurrent download request per file is supported.

This establishes that TDLib exposes recovery state and offset-based downloads, but it does not justify promising that every interrupted transfer can always resume from an arbitrary byte. The implementation must test actual file states and let TDLib validate whether the requested prefix is usable.

References:
- [localFile state](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1local_file.html)
- [getFileDownloadedPrefixSize](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1get_file_downloaded_prefix_size.html)
- [downloadFile](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1download_file.html)
- [TDLib maintainer clarification on concurrent ranges](https://github.com/tdlib/td/issues/3017)

## Priority and reordering limits (issue #18)

TDLib exposes a priority when starting a direct `downloadFile` request and when adding a message attachment with `addFileToDownloads`. The supported priority range is 1–32; higher values are scheduled earlier. For equal priorities, TDLib schedules the most recently requested item first.

The public API documents priority as an input to those start/add calls; it does not expose a dedicated operation for changing the priority of an already queued item. Therefore:

- Telestra can maintain an app-owned ordering for items that have not yet been submitted to TDLib, then submit them in that order.
- Map app-level priority bands to TDLib's 1–32 range when starting work.
- Do not promise an arbitrary drag-and-drop order for items already managed by TDLib's persistent download list.
- Do not remove and re-add active transfers merely to reorder them unless a later implementation proves that this preserves their progress and persistence.
- Equal-priority tie-breaking is TDLib-defined (last request first); the app must not claim FIFO ordering for ties.

This supports bounded priority scheduling, but not arbitrary live reordering of every active TDLib-managed download without additional app-side orchestration.

References:
- [downloadFile priority](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1download_file.html)
- [addFileToDownloads priority](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1add_file_to_downloads.html)

## Bandwidth limits (issue #19)

The pinned TDLib integration currently has no documented per-client bytes-per-second setting for either uploads or downloads in TDLib's public option catalog. TDLib does expose transfer progress and may notify when Telegram limits a user's upload or download speed; the Premium-related speedup options are informational and are not controls that Telestra can override.

Consequences:

- Do not promise a strict upload or download bandwidth cap through TDLib's JSON API.
- Lowering concurrency can reduce aggregate pressure, but it is not equivalent to a bytes-per-second limit.
- Delaying the start of additional transfers is an app-side scheduling policy, not a hard cap on transfers already running.
- Do not attempt to bypass Telegram/Premium server-side speed restrictions.
- A true per-direction bandwidth limiter would require a supported lower-level throttling hook or an external OS/network traffic-shaping mechanism. The latter is not appropriate as an implicit application behavior.

For now, treat separate upload/download bandwidth limits as unsupported by the current TDLib public API. Revisit only if TDLib adds a supported throttle control or the product explicitly accepts an external system-level integration.

References:
- [TDLib options catalog](https://core.telegram.org/tdlib/options)
- [TDLib updateSpeedLimitNotification](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1update_speed_limit_notification.html)
- [Telegram file transfer documentation](https://core.telegram.org/api/files)

## Automatic download rules (issue #21)

TDLib exposes native automatic-download settings per network type through `setAutoDownloadSettings(settings, type)`. The settings include an enable/disable switch and maximum sizes for photos, videos and other files, plus video/story/audio preloading preferences. TDLib's network types distinguish mobile, Wi-Fi and other connections.

This supports native per-connection and broad file-category rules. It does not expose a per-chat auto-download rule in this settings object.

Implementation direction:

- Use TDLib's settings API for the global mobile/Wi-Fi/other presets and photo/video/other-file size thresholds.
- Keep per-chat exceptions in Telestra's own settings and evaluate them before issuing a download request.
- Do not duplicate or silently override TDLib's global network presets when a chat-specific rule applies; define and test precedence explicitly.
- Treat unknown network type conservatively and fall back to the user's explicit "other" policy.
- Keep upload-video bitrate settings separate from download rules; they are not upload bandwidth caps.

References:
- [setAutoDownloadSettings](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1set_auto_download_settings.html)
- [autoDownloadSettings](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1auto_download_settings.html)
- [NetworkType](https://core.telegram.org/tdlib/docs/classtd_1_1td__api_1_1network_type.html)

## Current implementation status

A pure Rust queue model now exists in `src/telegram/transfers.rs`. It provides typed add/pause/resume/global-pause/remove intents, preserves insertion order, restores message-attachment entries from TDLib's initial list result without inventing an unknown priority, and applies authoritative file/list updates. Removal requests explicitly preserve the TDLib cache.

The model is not yet wired to the account worker's TDLib JSON requests or a GTK queue view. Direct-download cancellation is a separate API path and is not represented as a message-list pause command. Do not describe this as a complete transfer queue until those integration layers and end-to-end tests are implemented.

## Verification plan

- Unit-test queue state transitions with fake TDLib events: queued, active, paused, resumed, completed, canceled, and removed.
- Test global pause/resume without changing each item's identity or progress.
- Test pagination and initial list hydration before applying incremental updates. The decoder and model primitives now have unit coverage; end-to-end account startup ordering remains outstanding.
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
