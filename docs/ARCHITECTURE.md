# Architecture direction

**Status:** Initial proposal. This document records constraints and a direction, not a claim that these components have been implemented.

## Goals

Telestra should be a native-feeling Linux Telegram client with a clear separation between the desktop interface, application behavior and Telegram backend. GNOME is the primary design target, but the application should not require a GNOME session.

## Proposed layers

1. **Presentation** — GTK4 and libadwaita widgets, navigation, dialogs, accessibility and desktop integration.
2. **Application** — user workflows, state coordination, commands, account management and feature-level services.
3. **Telegram adapter** — a narrow Rust-facing interface for authentication, updates, chats, messages, files and other TDLib operations.
4. **TDLib runtime** — TDLib owns Telegram protocol details, synchronization and its managed local data.

UI components should not construct raw TDLib requests directly. The adapter should translate between application-level operations and TDLib types, and should make backend behavior testable without a live Telegram account wherever practical.

## TDLib integration

The initial integration should evaluate TDLib's JSON interface and the available Rust bindings before choosing an approach. The JSON interface is a reasonable interoperability baseline, but process/thread ownership, FFI safety, event delivery, error handling and packaging must be tested before committing to it.

TDLib is asynchronous. Its updates must be consumed without blocking GTK's main thread. Define explicit ownership and shutdown behavior for the TDLib client, event receiver and background tasks.

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

## Decisions still open

- TDLib JSON interface versus a maintained Rust binding or a small audited FFI layer.
- Minimum supported GTK4/libadwaita and Rust versions.
- Account data and cache locations, backup and migration policy.
- Translation/localization framework.
- Flatpak permissions and distribution strategy.
- Test strategy for TDLib integration and GUI behavior.

These decisions should be recorded here when supported by a small technical investigation or prototype.
