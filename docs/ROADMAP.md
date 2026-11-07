# Roadmap

This roadmap describes intended areas of work. It is not a promise that every item is feasible through TDLib or Telegram's public client API. Features should move to implementation only after their API support, privacy implications and maintenance cost are understood.

## Stage 0 — Project foundation

- [x] Establish project identity and initial documentation.
- [x] Record contributor and repository-agent guidance.
- [ ] Choose and document the TDLib integration strategy.
- [ ] Establish Rust, GTK4 and libadwaita version targets.
- [ ] Add a minimal application skeleton and reproducible development environment.
- [ ] Add formatting, linting, tests and CI.

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

## Release readiness

A release requires working authentication and core messaging, reliable session handling, documented data storage, dependency/license review, security review, automated tests, packaging, and clear known limitations. No release date is set.
