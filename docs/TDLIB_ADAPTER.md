# TDLib adapter implementation contract

**Status:** The transport-independent protocol, JSON envelope decoder, event routing, lifecycle primitives, background worker, isolated TDLib C ABI loader and native mailbox adapters are implemented and covered by tests. The application does not yet load the runtime at startup; native source-build verification is in progress, and Telegram authentication/connectivity is not available.

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
6. emitting application-owned `BackendEvent` values; GTK-facing code must receive them through a thread-safe handoff and dispatch them on the GLib main context.
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

The current tests cover the transport contract, request-ID generation and pending-request tracking, JSON envelope decoding, response/error correlation, update and authorization routing, lifecycle transitions, worker response correlation, rejected request envelopes, malformed-message protocol failures, send/receive failures and close confirmation. The native FFI loader and native sender/receive adapters are implemented. The pinned source-build workflow is in place but its first clean build is still being verified. Application startup wiring, GLib main-context handoff, Flatpak/multi-architecture packaging and end-to-end Telegram connectivity remain outstanding.


## Reproducible native build

The pinned source build is driven by `scripts/build-tdlib.sh`. On Linux, install a C++17 toolchain, CMake, Git, gperf, OpenSSL and zlib development files, then run:

```sh
scripts/build-tdlib.sh
```

The script fetches the exact commit in `packaging/tdlib/source.env`, rejects modified or mismatched source checkouts, builds only the shared `tdjson` target, installs it under `target/tdlib`, and verifies the required client-ID API symbols plus runtime dependencies. Optional positional arguments override the source checkout, install prefix and CMake build directory; `TDLIB_BUILD_JOBS` controls parallelism.

The dedicated TDLib workflow runs this build when its source pin, build scripts or workflow change. This verifies the current Linux host architecture only; x86_64/aarch64 package and Flatpak integration still require their own build and runtime checks.

## Native build gate

Do not enable a native runtime dependency until the pinned TDLib source has been built reproducibly for the supported architectures, the preferred `td_create_client_id`, `td_send` and `td_receive` symbols and runtime dependencies have been inspected, and native-package plus Flatpak smoke tests pass. See [Architecture](ARCHITECTURE.md) for the candidate source pin and packaging checklist.


## Shared receive dispatcher abstraction

The transport-independent dispatcher is implemented as `SharedDispatcher`. It owns a dedicated thread and a `SharedReceiveSource`; only that thread polls the source. The source contract requires bounded receives and owned message strings, allowing the native TDLib adapter to copy its borrowed C response before polling again.

Account registration is serialized through dispatcher commands. Each account receives a dedicated FIFO mailbox, and routing failures or native receive failures are surfaced through the dispatcher's event receiver rather than silently discarded. The abstraction is tested with a channel-backed source. `NativeTdLibReceiveSource` now connects it to the isolated C ABI loader and copies each native response into an owned string.


## Account worker transport

`AccountMailboxTransport` connects the existing account worker to a registered mailbox and a client-ID-aware request sender. It routes outgoing requests through the sender with the account's TDLib client ID, consumes only that account's mailbox, and sends the TDLib `close` request once when the worker begins shutdown. `NativeTdLibRequestSender` sends through the preferred `td_send(client_id, request)` API, while `NativeTdLibReceiveSource` is the only source that should be supplied to the process-wide dispatcher. Both share the loaded library handle.

## Native adapter composition

Load the pinned shared library once and share its handle between the request sender and receive dispatcher. Allocate a client ID and register its mailbox before sending the first request, so the first TDLib response cannot arrive before its route exists:

```rust
let library = TdJsonLibrary::load("libtdjson.so")?;
let dispatcher = SharedDispatcher::spawn(NativeTdLibReceiveSource::new(library.clone()))?;
let client_id = library.create_client_id();
let mailbox = dispatcher.register_client(client_id)?;
let sender = Arc::new(NativeTdLibRequestSender::new(library));
let transport = AccountMailboxTransport::new(client_id, sender, mailbox);
```

The account worker owns `transport`; after it receives `authorizationStateClosed`, unregister that client from the dispatcher. The process-wide dispatcher must remain alive until every account has closed.
