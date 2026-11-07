# Telestra

A native, feature-rich Telegram client for Linux, built with Rust, GTK4, libadwaita and TDLib.

Telestra aims to combine a GNOME-native desktop experience with powerful tools for managing chats, media, files and multiple accounts. It is an independent client, not a fork of Telegram Desktop.

## Project status

**Early planning / initial development.** The application is not ready for everyday use. Features, architecture and release targets are still being established.

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

- **Language:** Rust
- **UI:** GTK4 and libadwaita
- **Telegram client library:** TDLib
- **Packaging target:** Flatpak

## Development

Development instructions will be added as the build system, TDLib integration and application structure are established. Do not treat this repository as buildable yet.

## License

The project is intended to use **GPL-3.0-or-later**. See [LICENSE](LICENSE) once the license file is added.
