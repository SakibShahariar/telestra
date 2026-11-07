# Repository guidance

## Project identity

Telestra is an independent, native Linux Telegram client built with Rust, GTK4, libadwaita and TDLib. The product should feel at home on GNOME while remaining usable on other Linux desktops.

The project is in its initial planning stage. Do not describe planned functionality as implemented, and do not introduce scaffolding that implies an application already exists.

## Working rules

- Work directly on the verified default branch unless the maintainer explicitly authorizes another workflow.
- Inspect the current repository state before making changes. Fetch the current contents and blob SHA of every existing file immediately before editing it.
- Re-check the default branch head before each commit. Never overwrite concurrent changes.
- Do not force-push, rewrite history, amend published commits, create unapproved branches, or open pull requests unless explicitly requested.
- Keep commits focused and use conventional commit subjects, for example `feat:`, `fix:`, `docs:`, `build:`, `test:`, and `refactor:`.
- Preserve existing architecture, public interfaces, dependencies, formatting and maintainer decisions unless a change is justified.
- Prefer small, reviewable changes. Do not add dependencies without a clear need and an explanation of the trade-off.
- Verify relevant tests, formatting, linting and CI results. Never claim a check passed unless it was actually run and its result inspected.
- Do not close an issue merely because documentation mentions it; verify that its acceptance criteria are implemented.

## Technical direction

- Use Rust for application logic and GTK4/libadwaita for the desktop interface.
- Use TDLib as the Telegram client backend. Keep Telegram-specific transport and update handling behind a narrow adapter so UI code does not depend on raw TDLib details.
- Keep network and file operations asynchronous; never block the GTK main thread.
- Treat TDLib's actual API and documented server capabilities as authoritative. Do not promise unsupported concurrency, transfer acceleration, privacy bypasses, or server-side Premium entitlements.
- Keep UI components accessible: keyboard navigation, screen-reader labels, scalable layouts, reduced-motion considerations and RTL support.
- Follow GNOME Human Interface Guidelines where appropriate without making the application unusable on non-GNOME desktops.
- Store secrets carefully. Never commit API credentials, account data, session databases, access tokens or personal information.
- Clearly distinguish local-only enhancements from Telegram server features.

## Documentation and scope

- Keep the README concise and accurate.
- Record substantial architectural decisions and their rationale in project documentation.
- Mark roadmap items as planned, in progress or complete only when evidence supports the status.
- Before implementing a feature, check TDLib support, Telegram API constraints, licensing, security and accessibility implications.
