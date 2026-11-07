# Contributing to Telestra

Thanks for your interest in Telestra. The project is at an early stage, so discussion and careful design are especially valuable.

## Before contributing

- Read [README.md](README.md), [AGENTS.md](AGENTS.md), and the relevant documents in [docs/](docs/).
- Check existing issues and discussions before starting substantial work.
- For major features or architectural changes, open an issue first so the scope can be agreed before implementation.
- Do not assume a roadmap item is already approved for implementation merely because it is listed.

## Development principles

- Keep the application native to GTK4 and libadwaita.
- Keep Telegram backend concerns separate from UI code.
- Prefer asynchronous operations and responsive interfaces.
- Preserve accessibility, keyboard usability, localization readiness and support for non-GNOME Linux desktops.
- Avoid speculative dependencies and unnecessary abstractions.
- Never commit credentials, private account data, session files or local build artifacts.

## Changes

- Make focused changes with clear intent.
- Add or update tests when behavior changes.
- Update documentation when commands, behavior, configuration or architecture changes.
- Use conventional commit subjects such as `feat:`, `fix:`, `docs:`, `build:`, `test:` and `refactor:`.
- Include an issue number in the commit subject when the change addresses a tracked issue.

## Validation

Run the formatting, linting and test commands documented by the project once a build system is available. Report checks that could not be run and why; do not imply unrun checks passed.

## Reporting a bug

Include the application version or commit, Linux distribution and desktop environment, steps to reproduce, expected behavior, actual behavior and relevant logs. Remove account identifiers, phone numbers, message contents, tokens and other private information before sharing logs.

## Proposing a feature

Describe the user problem, expected behavior, relevant Telegram/TDLib API support, privacy and security implications, accessibility impact and possible alternatives. Separate local client-side enhancements from features that require Telegram server support.
