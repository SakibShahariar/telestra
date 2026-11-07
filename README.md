# Telestra

A native, feature-rich Telegram client for Linux, built with Rust, GTK4, libadwaita and TDLib.

Telestra aims to combine a GNOME-native desktop experience with powerful tools for managing chats, media, files and multiple accounts. It is an independent client, not a fork of Telegram Desktop.

## Project status

**Early development.** The initial GTK4/libadwaita application shell is in place, but Telegram connectivity, authentication and messaging are not implemented. Telestra is not ready for everyday use.

## Planned direction

- Native GTK4 and libadwaita interface, following GNOME design conventions.
- Telegram functionality powered by TDLib.
- Multiple-account and advanced chat-management workflows.
- A capable download center with queues, priorities, retries and controls.
- A unified, searchable media and file library.
- Privacy, accessibility and keyboard-driven power-user features.
- Flatpak packaging and support for Linux desktops beyond GNOME.

This list describes project goals, not implemented features. Telegram server-side Premium entitlements remain subject to Telegram's own account and subscription rules.

## Technology

- **Language:** Rust 2024 (Rust 1.85 or newer)
- **UI:** GTK4 and libadwaita Rust bindings
- **Telegram client library:** TDLib (integration adapter under evaluation)
- **Packaging target:** Flatpak

## Development

Install Rust 1.85 or newer and the GTK4/libadwaita development libraries for your distribution, then run:

```sh
cargo run
```

Useful checks:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

See [CONTRIBUTING.md](CONTRIBUTING.md) before proposing changes. The first window is only a UI shell; it does not connect to Telegram.

## Project documentation

- [Architecture direction](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)
- [Contributing](CONTRIBUTING.md)
- [Repository guidance](AGENTS.md)

## License

Telestra is licensed under **GPL-3.0-or-later**. See [LICENSE](LICENSE). Third-party components retain their own licenses.
