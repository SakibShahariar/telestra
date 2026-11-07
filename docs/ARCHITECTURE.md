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

**Rust transport implementation: still under evaluation.** The `tdlib-rs` crate is a promising candidate: its 1.4.0 release documents support for TDLib 1.8.61, generated Rust types, and build modes including precompiled Linux x86_64/ARM64 libraries. It is not adopted yet. Before selecting it, a small integration spike must verify:
- Build reproducibility on the supported Linux targets.
- How the native TDLib library is packaged and discovered at runtime, especially in Flatpak.
- License compatibility and the exact upstream maintenance/release process.
- Client ownership, update delivery, cancellation and shutdown behavior.
- Whether the wrapper exposes the TDLib methods needed for transfers without hiding important capabilities.

If the wrapper cannot meet those requirements, use the JSON interface through a small audited FFI layer rather than coupling the rest of the application to a third-party wrapper.

TDLib remains asynchronous. Its updates must be consumed without blocking GTK's main thread. Background runtime ownership and shutdown must be explicit, and GTK-facing state changes must be marshalled onto the main context.

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
