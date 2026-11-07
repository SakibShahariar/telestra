# Roadmap

This roadmap describes intended areas of work. It is not a promise that every item is feasible through TDLib or Telegram's public client API. Features should move to implementation only after their API support, privacy implications and maintenance cost are understood.

## Stage 0 — Project foundation

- [x] Establish project identity and initial documentation.
- [x] Record contributor and repository-agent guidance.
- [x] Define the TDLib JSON interface boundary and document the Rust transport candidates; final wrapper choice awaits a build/runtime spike.
- [x] Establish Rust, GTK4 and libadwaita Rust-binding version targets.
- [ ] Integrate the TDLib JSON runtime and native-library packaging strategy ([#145](https://github.com/SakibShahariar/telestra/issues/145)).
- [x] Add a minimal application skeleton and reproducible development environment (GTK4/libadwaita shell compiles; CI validation passed).
- [x] Add formatting, linting, tests and CI (formatting, Clippy and tests passed in CI run #13).

## Stage 1 — Essential Telegram client

- [ ] TDLib initialization, shutdown and error handling.
- [ ] Authentication and session lifecycle.
- [ ] Account and chat list.
- [ ] Message history, sending and receiving.
- [ ] Basic attachments and file transfers.
- [ ] Notifications and unread state.
- [ ] Search and basic chat management.

## Stage 2 — Download center

- [ ] Transfer queue with accurate progress and status.
- [ ] Pause, resume and cancel where supported.
- [ ] Retry and recovery behavior.
- [ ] Queue priorities and configurable rules.
- [ ] Bandwidth controls and scheduling, subject to backend support.
- [ ] Bulk transfer workflows and clear destination management.

## Stage 3 — Power-user workflows

- [ ] Multiple accounts.
- [ ] Chat folders, filters and quick switching.
- [ ] Advanced search and multi-select actions.
- [ ] Keyboard shortcuts and command palette.
- [ ] Scheduled-message management where supported.

## Stage 4 — Media library

- [ ] Searchable media and file views across chats.
- [ ] Metadata filters and date-based navigation.
- [ ] Bulk save and organization.
- [ ] Cache inspection and cleanup controls.

## Stage 5 — Privacy and customization

- [ ] Application lock and automatic locking.
- [ ] Notification privacy and hidden previews.
- [ ] Screen-sharing privacy mode.
- [ ] Theme, density and layout customization.
- [ ] Local bookmarks and other client-only enhancements.

## Stage 6 — Network, accessibility and polish

- [ ] Proxy profile management using supported TDLib capabilities.
- [ ] Network diagnostics and connection-state visibility.
- [ ] Accessibility and RTL audit.
- [ ] Performance and memory profiling.
- [ ] Localization and desktop integration improvements.

## Feature tracking

The detailed feature backlog is organized into 14 parent issues. Each parent links to independently trackable child issues. These are planning items, not implemented features; API-dependent work must be validated before implementation.

1. [Download and upload performance](https://github.com/SakibShahariar/telestra/issues/1)
2. [Messaging and message workflows](https://github.com/SakibShahariar/telestra/issues/2)
3. [Privacy and discreet usage](https://github.com/SakibShahariar/telestra/issues/3)
4. [Chat management and organization](https://github.com/SakibShahariar/telestra/issues/4)
5. [Media library and viewer](https://github.com/SakibShahariar/telestra/issues/5)
6. [Translation and optional AI tools](https://github.com/SakibShahariar/telestra/issues/6)
7. [Proxy and network management](https://github.com/SakibShahariar/telestra/issues/7)
8. [Premium-like local enhancements](https://github.com/SakibShahariar/telestra/issues/8)
9. [Desktop UI and GNOME integration](https://github.com/SakibShahariar/telestra/issues/9)
10. [Notifications](https://github.com/SakibShahariar/telestra/issues/10)
11. [Advanced tools and utilities](https://github.com/SakibShahariar/telestra/issues/11)
12. [Performance and resource efficiency](https://github.com/SakibShahariar/telestra/issues/12)
13. [Security and local data protection](https://github.com/SakibShahariar/telestra/issues/13)
14. [Accessibility and internationalization](https://github.com/SakibShahariar/telestra/issues/14)

## Release readiness

A release requires working authentication and core messaging, reliable session handling, documented data storage, dependency/license review, security review, automated tests, packaging, and clear known limitations. No release date is set.
