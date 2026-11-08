# Telestra

A native Linux Telegram client built with Rust, GTK4, libadwaita and TDLib.

Telestra is an independent desktop client designed around GNOME conventions, while aiming to remain usable on other Linux desktops. It is not a fork of Telegram Desktop.

## Status

**Early development.**

The GTK4/libadwaita application shell and parts of the backend foundation are in place. **Telegram login, account connectivity and messaging are not yet available**, so Telestra is not ready for everyday use.

## What we're building

- **Chats and messaging** — accounts, conversations, message workflows and search.
- **Transfers** — a download center with queue management, progress and recovery controls.
- **Media library** — searchable media and files, with filters and bulk actions.
- **Power-user tools** — chat organization, keyboard workflows and multiple accounts.
- **Privacy and accessibility** — local-data controls, discreet notifications, keyboard and assistive-technology support.
- **Linux integration** — a native GTK experience, with Flatpak packaging and support beyond GNOME.

These are project goals, not claims about features available today. See the [roadmap](docs/ROADMAP.md) for progress and the [architecture notes](docs/ARCHITECTURE.md) for technical decisions.

## Development

### Requirements

- Rust 1.85 or newer
- GTK4 and libadwaita development libraries

Run the application shell:

```sh
cargo run
```

Run the project checks:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

The current application does not connect to Telegram. Read [CONTRIBUTING.md](CONTRIBUTING.md) before making changes.

## Documentation

- [Roadmap](docs/ROADMAP.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Contributing](CONTRIBUTING.md)
- [Repository guidance](AGENTS.md)

## License

[GPL-3.0-or-later](LICENSE). Third-party components retain their respective licenses.
