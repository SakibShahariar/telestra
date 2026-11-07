# TDLib adapter implementation contract

**Status:** Transport-independent protocol, JSON envelope decoder, event routing, lifecycle primitives and a generic background worker are implemented and covered by unit tests. No native TDLib library is linked, and Telegram connectivity is not available.

## Ownership

Each Telestra account receives a client ID from `td_create_client_id`. The preferred API sends requests with `td_send(client_id, request)` and delivers all accounts' updates and responses through the process-wide `td_receive(timeout)` stream. The pinned header explicitly marks `td_json_client_*` as the old alternative interface, scheduled for removal in TDLib 2.0.0; do not use it.

A single process-wide dispatcher must be the only caller of native `td_receive`. It copies each returned C string before the next receive call, preserves global arrival order, reads `@client_id`, and forwards the owned JSON message to the matching account's mailbox. The account-level `JsonTransport` is a logical mailbox transport, not a direct wrapper around a native per-client receive call. Account workers consume only their own mailbox and send through a client-ID-aware dispatcher. TDLib automatically destroys a client after it is closed; the deprecated interface's explicit `td_json_client_destroy` is not part of this design.

## Worker responsibilities

`BackendWorker` in `src/telegram/worker.rs` owns its `JsonTransport` on a dedicated thread and is the only caller of its receive operation. It never touches GTK; the application must forward its events to the GLib main context. It is responsible for:

1. Accepting application commands and serializing TDLib requests.
2. Adding a unique Telestra `@extra` to each request that expects a response.
3. Inserting pending operations before sending requests, so an immediate response cannot race registration.
4. Receiving and decoding every response/update in TDLib arrival order.
5. Resolving only the pending request whose canonical `@extra` matches a response.
6. amitting application-owned `BackendEvent` values; GTK-facing code must receive them through a thread-safe handoff and dispatch them on the GLib main context.
7. Draining pending operations with an interruption/error result if the receive loop fails or the client shuts down.

The event model distinguishes correlated responses, unsolicited updates, authorization-state changes, TDLib errors and transport failures. The dependency-free envelope decoder in `src/telegram/decoder.rs` validates complete JSON objects, extracts `@type` and string `@extra`, decodes TDLib error objects and authorization-state updates, and retains original JSON for ordinary responses and updates. It supports nested arrays/objects, escaped Unicode, and all valid JSON number forms while rejecting malformed input, duplicate keys and trailing data. It is an envelope decoder, not a complete generated TDLib schema. Unknown authorization states must be retained rather than causing a panic or silently changing state.

## Lifecycle and shutdown

`ClientState` models `Created -> Running -> Closing -> Closed`, with an unrecoverable `Failed` terminal state. The transition to `Closing` records that a close request has been sent; it does not authorize destroying the native client.

On shutdown, the worker must:

1. Stop accepting new application requests.
2. Send TDLib's `close` request.
3. Continue receiving and dispatching updates.
4. Wait until the decoded authorization state is `authorizationStateClosed`.
5. Mark the lifecycle `Closed` and release the account mailbox/routing state. TDLib automatically destroys the client after it is closed; the preferred API has no explicit destroy call.
6. Resolve or fail all outstanding application operations.

A transport error during shutdown must be surfaced and must not be misreported as a clean TDLib close. Tests must cover both the normal close confirmation and failure path.

## Test boundary

The transport, lifecycle, request correlation, decoder and event types are deliberately independent of a real Telegram account. Existing unit tests cover FIFO transport behavior, request correlation, malformed JSON rejection, TDLib error conversion, future authorization states, and preservation of pending requests across updates. As the worker is implemented, extend fake-transport tests to cover:

- FIFO delivery of updates and responses.
- Send ordering and request registration before send.
- Unknown or malformed `@extra` values not consuming pending operations.
- TDLib error code/message conversion.
- Unknown future authorization states.
- Close request followed by continued receive until the closed authorization state.
- Receive-loop failure and clean shutdown both draining pending operations.
- One receive owner per client and no GTK access from the worker.

The current tests cover the transport contract, request-ID generation and pending-request tracking, JSON envelope decoding, response/error correlation, update and authorization routing, lifecycle transitions, worker response correlation, rejected request envelopes, receive failures and close confirmation. Native FFI, native package integration, GLib main-context handoff and end-to-end Telegram connectivity remain unimplemented.

## Native build gate

Do not enable a native runtime dependency until the pinned TDLib source has been built reproducibly for the supported architectures, the preferred `td_create_client_id`, `td_send` and `td_receive` symbols and runtime dependencies have been inspected, and native-package plus Flatpak smoke tests pass. See [Architecture](ARCHITECTURE.md) for the candidate source pin and packaging checklist.
