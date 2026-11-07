# Architecture direction

**Status:** Initial architecture with a minimal GTK4/libadwaita shell. The Telegram backend is not yet connected.

## Goals

Telestra should be a native-feeling Linux Telegram client with a clear separation between the desktop interface, application behavior and Telegram backend. GNOME is the primary design target, but the application should not require a GNOME session.

## Proposed layers

1. **Presentation** — GTK4 and libadwaita widgets, navigation, dialogs, accessibility and desktop integration.
2. **Application** — user workflows, state coordination, commands, account management and feature-level services.
3. **Telegram adapter** — a narrow Rust-facing interface for authentication, updates, chats, messages, files and other TDLib operations.
4. **TDLib runtime** — TDLib owns Telegram protocol details, synchronization and its managed local data.

UI components should not construct raw TDLib requests directly. The adapter should translate between application-level operations and application-owned types, and should make backend behavior testable without a live Telegram account wherever practical.

## TDLib integration decision

**Protocol boundary: TDLib JSON interface.** TDLib's official documentation recommends the JSON interface for most use cases and describes the API as fully asynchronous. Requests and responses are correlated with the `@extra` field, while updates carry essential state changes. The adapter must therefore own request correlation, update dispatch, error conversion and orderly shutdown; none of these details belong in GTK widgets.

**C ABI decision: use TDLib's preferred client-ID JSON interface, not its deprecated client-scoped wrapper.** Inspection of `td/telegram/td_json_client.h` at the pinned TDLib commit shows that the primary interface is `td_create_client_id`, `td_send`, and process-wide `td_receive`. The header explicitly labels `td_json_client_create/send/receive/destroy` as the old alternative interface and says it will be removed in TDLib 2.0.0. The earlier architecture note had these roles reversed; do not implement or link the deprecated `td_json_client_*` API.

The preferred API changes the receive topology: TDLib exposes one process-wide incoming stream, not an independent receive queue per account. Telestra must have exactly one native receive dispatcher for the process. That dispatcher owns calls to `td_receive`, copies each borrowed C string immediately, preserves arrival order, reads `@client_id`, and forwards each event to the matching account's mailbox. Account workers may remain isolated and single-consumer, but their `receive` operation reads from those mailboxes; it must never call TDLib's global `td_receive` itself. Requests are sent with `td_send(client_id, json)`. TDLib destroys a client automatically after it is closed, so do not call the deprecated interface's `td_json_client_destroy`.

The `tdlib-rs 1.4.0` crate is not selected for the runtime until its process-wide dispatcher and client lifecycle can be audited against this topology. A small FFI boundary should expose safe client-ID allocation, request sending, and a single receive-dispatcher API, with all unsafe code and C-string handling isolated there. TDLib itself will be built and packaged separately from the GTK application, pinned to an explicit upstream revision. Before enabling the runtime in normal builds, verify:
- Reproducible TDLib builds for each supported Linux architecture.
- Native package and Flatpak integration, including library discovery and runtime dependencies.
- The exported `td_create_client_id`, `td_send`, and `td_receive` symbols in the packaged library.
- One process-wide receive owner, ordered event routing by `@client_id`, request correlation, and orderly close behavior.
- License compatibility and the upstream security/update process.

The adapter will therefore call the modern TDLib C JSON API through a narrowly scoped FFI module, keeping all unsafe code and C-string handling inside that boundary. The FFI layer will expose safe Rust operations and owned JSON strings to the rest of the application. TDLib itself will be built and packaged separately from the GTK application, pinned to an explicit upstream revision. Before enabling the runtime in normal builds, verify:
- Reproducible TDLib builds for each supported Linux architecture.
- Native package and Flatpak integration, including library discovery and runtime dependencies.
- The exported `td_create_client_id`, `td_send`, and `td_receive` symbols in the packaged library.
- Client ownership, ordered update delivery, request correlation, cancellation and shutdown.
- License compatibility and the upstream security/update process.

The `tdlib-rs` build modes are not a shortcut around this packaging work: its download mode fetches a release archive during Cargo builds, its dynamic mode uses a build-output RPATH, and its static mode requires additional C++ runtime libraries on Linux.

The current `tdlib-rs 1.4.0` build script adds specific packaging risks that must be tested rather than assumed away: its `download-tdlib` mode downloads a release archive during Cargo's build; its dynamic mode embeds an RPATH pointing into Cargo's `OUT_DIR`; and its Linux static mode links the C++ runtime libraries `c++` and `c++abi` in addition to TDLib's static libraries. A Flatpak build would need a pinned, offline-available native-library input and a verified runtime/linking arrangement. These details make the crate a candidate for a prototype, not yet an approved production dependency.

TDLib remains asynchronous. Its updates must be consumed without blocking GTK's main thread. Background runtime ownership and shutdown must be explicit, and GTK-facing state changes must be marshalled onto the main context.

### Native TDLib build and packaging direction

The initial packaging direction is to build TDLib from an explicitly pinned upstream source revision for each supported target, rather than downloading opaque third-party binaries during a Cargo build. Use TDLib's CMake-provided JSON interface target (Td::TdJson for the shared library, or Td::TdJsonStatic only if static-linking requirements are deliberately accepted). Keep the source revision, build options and native dependency versions identical between native-package and Flatpak builds.

The initial upstream candidate is TDLib 1.8.69, pinned to the exact upstream commit `0e88653dcbf4e44c73cb655a05b58cd749a91169` (the commit that updates the upstream project version to 1.8.69). The upstream repository does not publish GitHub release assets, so the full commit SHA—not a moving branch name or a third-party binary bundle—is the source identity to evaluate. This is a candidate pin only; update it deliberately after reviewing upstream changes and rerunning the packaging matrix.

The upstream build documentation identifies a C++17 compiler, OpenSSL, zlib, CMake and gperf (build-time) as the core dependencies. Its CMake build exposes dedicated JSON interface targets. For a low-memory build, upstream documents building the tdjson target after source preparation. This makes a source-based build reproducible in principle, but does not yet prove that our Flatpak manifest, native packages or supported architecture matrix work.

Before treating this direction as final:
- Pin an exact TDLib commit and record the resolved source archive checksum.
- Build the JSON shared library for x86_64 and aarch64 using the same documented options.
- Verify the output exports `td_create_client_id`, `td_send`, and `td_receive`, and record its runtime dependencies.
- Verify that the Flatpak build is network-independent after source acquisition and installs the library under the app's controlled runtime path.
- Run a clean CI link-and-start test against the produced library, plus a package-level smoke test for both architectures.
- Decide whether native distributions should prefer their system TDLib package or bundle the pinned library only after ABI and symbol compatibility are checked.

Until those checks pass, this is a packaging direction, not a completed integration or a claim that a TDLib runtime can already be built by CI.

### Preferred JSON API and receive topology

Use the preferred client-ID interface from the pinned TDLib header:

- `int td_create_client_id(void)` allocates an account client ID.
- `void td_send(int client_id, const char *request)` sends a request for that account.
- `const char *td_receive(double timeout)` receives the next event for any client.
- `const char *td_execute(const char *request)` is reserved for requests TDLib explicitly documents as synchronously executable.

The `td_json_client_create/send/receive/execute/destroy` family is explicitly described in that header as the old alternative interface and is scheduled for removal in TDLib 2.0.0. Do not use it.

- Run exactly one process-wide native receive dispatcher. TDLib forbids simultaneous calls to `td_receive`; per-account workers must never call it directly.
- The dispatcher copies each returned C string into owned Rust memory before the next `td_receive` or `td_execute` call, parses `@client_id`, and forwards the message to the matching account mailbox. Preserve global receive order while routing.
- Each account worker may own a single-consumer mailbox transport and send through a client-ID-bound sender. Its receive operation is mailbox consumption, not a native TDLib receive call.
- Attach a unique `@extra` value to every request that expects a response and route the matching response to the waiting application operation. Updates without a matching `@extra` remain ordered events, not failed requests.
- Shutdown sends TDLib's `close` request with `td_send`, continues receiving through the shared dispatcher until `updateAuthorizationState` reports `authorizationStateClosed` for that client, and then releases account-side routing state. The preferred API destroys the TDLib client automatically after it is closed; do not call `td_json_client_destroy`.
- GTK-facing events must cross a thread-safe channel and be dispatched on the GLib main context; no GTK objects may be touched by the native receive dispatcher or account workers.

These constraints should shape the fake-transport tests before any live-account authentication is introduced.


The adapter's detailed ownership, worker, event and shutdown contract is maintained in [TDLib adapter implementation contract](TDLIB_ADAPTER.md). The transport-independent protocol, decoder, event routing, lifecycle primitives and generic transport-owning worker are implemented; native FFI, GLib main-context handoff and package integration remain outstanding.

## Rust and desktop dependency baseline

- Rust edition: 2024; minimum Rust version: 1.85.
- GTK4 Rust bindings: 0.11.x.
- libadwaita Rust bindings: 0.9.x.
- CI baseline: Ubuntu 24.04 with GTK4 and libadwaita development packages.
- Use the gtk-rs/libadwaita versions together; their GLib-family dependencies must remain compatible.

The application shell currently uses these bindings only. No TDLib dependency is linked yet.

## Data and security

- Do not duplicate TDLib-managed account state without a demonstrated product need.
- Keep application preferences separate from Telegram session storage.
- Treat message content, downloaded files, local indexes and logs as sensitive user data.
- Do not log authentication codes, access tokens, encryption keys or message contents by default.
- Document the location and lifecycle of local data before release.
- Any additional local database or search index must have explicit retention, cleanup and backup behavior.

## Downloads and media

The download manager should be designed as an application-level queue over the transfer capabilities TDLib actually exposes. Concurrency, retry, resume, scheduling and bandwidth controls must be validated against the API rather than assumed. The UI should expose real transfer state and must not claim speed improvements that the backend cannot provide.

A unified media library may index metadata from chats and local files, but should not silently copy or retain media beyond the user's configured policy.

## UI principles

- Follow GNOME Human Interface Guidelines and libadwaita conventions.
- Support keyboard navigation, screen readers, text scaling, high contrast and reduced motion.
- Keep layouts adaptive and avoid GNOME-only assumptions in core workflows.
- Use platform-native notifications and desktop integration where available, with graceful fallbacks.

## Remaining decisions

- Final TDLib Rust transport choice after the integration spike.
- Exact minimum system GTK4/libadwaita versions and supported Linux distributions.
- Account data and cache locations, backup and migration policy.
- Translation/localization framework.
- Flatpak permissions and distribution strategy.
- Test strategy for TDLib integration and GUI behavior.

Record further decisions here when supported by a small technical investigation or prototype.

## Technical references

- [TDLib getting started](https://core.telegram.org/tdlib/getting-started) — asynchronous request/response flow, updates and authorization.
- [TDLib documentation](https://core.telegram.org/tdlib/docs/) — JSON interface recommendation and API reference.
- [tdlib-rs 1.4.0 API](https://docs.rs/tdlib-rs/1.4.0/tdlib_rs/) — investigated wrapper API; not selected until its process-wide receive dispatcher and client-ID routing are audited.
- [Pinned TDLib JSON C header](https://github.com/tdlib/td/blob/0e88653dcbf4e44c73cb655a05b58cd749a91169/td/telegram/td_json_client.h) — authoritative API declarations and explicit status of the old `td_json_client_*` alternative.
- [gtk4 Rust bindings](https://docs.rs/gtk4/latest/gtk4/) — GTK4 Rust API and application lifecycle.
- [libadwaita Rust bindings](https://docs.rs/libadwaita/latest/libadwaita/) — libadwaita application and window patterns.
