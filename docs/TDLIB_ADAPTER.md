# TDLib adapter implementation contract

**Status:** Design and transport-independent primitives only. No native TDLib library is linked, and this document does not claim that Telegram connectivity works.

## Ownership

Each Telestra account owns one client-scoped TDLib JSON client. The eventual native adapter must use `td_json_client_create`, `td_json_client_send`, `td_json_client_receive` and `td_json_client_destroy`. Do not use the deprecated process-global client-ID interface.

The `JsonTransport` trait in `src/telegram/transport.rs` is the Rust boundary for that client. It requires mutable access for sends, receives and close requests, returns owned response strings, and documents the single-receiver and message-order guarantees. The native implementation must copy the TDLib response bytes before calling receive or execute again.

## Worker responsibilities

The backend worker—not GTK—must own the transport and be the only caller of its receive operation. It is responsible for:

1. Accepting application commands and serializing TDLib requests.
2. Adding a unique Telestra `@extra` to each request that expects a response.
3. Inserting pending operations before sending requests, so an immediate response cannot race registration.
4. Receiving and decoding every response/update in TDLib arrival order.
5. Resolving only the pending request whose canonical `@extra` matches a response.
6. Emitting application-owned `BackendEvent` values; GTK-facing code must receive them through a thread-safe handoff and dispatch them on the GLib main context.
7. Draining pending operations with an interruption/error result if the receive loop fails or the client shuts down.

The event model distinguishes correlated responses, unsolicited updates, authorization-state changes, TDLib errors and transport failures. The dependency-free envelope decoder in `src/telegram/decoder.rs` validates complete JSON objects, extracts `@type` and string `@extra`, decodes TDLib error objects and authorization-state updates, and retains original JSON for ordinary responses and updates. It supports nested arrays/objects, escaped Unicode, and all valid JSON number forms while rejecting malformed input, duplicate keys and trailing data. It is an envelope decoder, not a complete generated TDLib schema. Unknown authorization states must be retained rather than causing a panic or silently changing state.

## Lifecycle and shutdown

`ClientState` models `Created -> Running -> Closing -> Closed`, with an unrecoverable `Failed` terminal state. The transition to `Closing` records that a close request has been sent; it does not authorize destroying the native client.

On shutdown, the worker must:

1. Stop accepting new application requests.
2. Send TDLib's `close` request.
3. Continue receiving and dispatching updates.
4. Wait until the decoded authorization state is `authorizationStateClosed`.
5. Mark the lifecycle `Closed`, destroy the native client exactly once, and join the worker.
6. Resolve or fail all outstanding application operations.

A transport error during shutdown must be surfaced and must not be misreported as a clean TDLib close. Tests must cover both the normal close confirmation and failure path.

## Test boundary

The transport, lifecycle, request correlation and event types are deliberately independent of a real Telegram account. Fake-transport tests should cover:

- FIFO delivery of updates and responses.
- Send ordering and request registration before send.
- Unknown or malformed `@extra` values not consuming pending operations.
- TDLib error code/message conversion.
- Unknown future authorization states.
- Close request followed by continued receive until the closed authorization state.
- Receive-loop failure draining pending operations.
- One receive owner per client and no GTK access from the worker.

The current tests exercise only the foundational transport contract, lifecycle transitions, typed event values and request correlation. The worker, JSON decoder, native FFI, package integration and full shutdown handshake remain unimplemented.

## Native build gate

Do not enable a native runtime dependency until the pinned TDLib source has been built reproducibly for the supported architectures, the required client-scoped symbols and runtime dependencies have been inspected, and native-package plus Flatpak smoke tests pass. See [Architecture](ARCHITECTURE.md) for the candidate source pin and packaging checklist.
