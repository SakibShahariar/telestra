# Repository guidance

## Project identity

Telestra is an independent, native Linux Telegram client built with Rust, GTK4, libadwaita and TDLib. The product should feel at home on GNOME while remaining usable on other Linux desktops.

The repository now contains an initial GTK4/libadwaita application shell and a documented architecture, but Telegram connectivity, authentication and messaging are not implemented. Keep all status claims aligned with the actual code; do not describe planned functionality as implemented.

## Working rules

- Work directly on the verified default branch unless the maintainer explicitly authorizes another workflow.
- Inspect the current repository state before making changes. Fetch the current contents and blob SHA of every existing file immediately before editing it.
- Re-check the default branch head before each commit. Never overwrite concurrent changes.
- Do not force-push, rewrite history, amend published commits, create unapproved branches, or open pull requests unless explicitly requested.
- Keep commits focused and use conventional commit subjects, for example `feat:`, `fix:`, `docs:`, `build:`, `test:`, and `refactor:`. When a commit addresses a GitHub issue, include its reference in the subject, such as `feat(downloads): add transfer queue (#15)`. Include multiple issue references only when the commit genuinely addresses each one.
- Preserve existing architecture, public interfaces, dependencies, formatting and maintainer decisions unless a change is justified.
- Prefer small, reviewable changes. Do not add dependencies without a clear need and an explanation of the trade-off.
- Verify relevant tests, formatting, linting and CI results. Never claim a check passed unless it was actually run and its result inspected.
- Do not close an issue merely because documentation mentions it; verify that its acceptance criteria are implemented.

## Technical direction

- Use Rust for application logic and GTK4/libadwaita for the desktop interface.
- Use TDLib as the Telegram client backend. Keep Telegram-specific transport and update handling behind a narrow adapter so UI code does not depend on raw TDLib details.
- Keep the application's `unsafe_code = "forbid"` lint in force. Put TDLib C ABI declarations, pointer ownership and C-string conversion in a dedicated, narrowly scoped FFI crate; expose only safe Rust operations and owned response strings to the application crate.
- The TDLib `td_receive` pointer is borrowed only until the next receive/execute call on the receiving thread. Copy it into owned memory immediately. The preferred client-ID API has one process-wide receive stream: exactly one dispatcher may call `td_receive`, and it must route messages by `@client_id` while preserving arrival order.
- Keep network and file operations asynchronous; never block the GTK main thread.
- Keep the TDLib JSON receive stream single-owner: one process-wide dispatcher calls `td_receive`, routes owned messages by `@client_id`, and preserves arrival order. Account workers consume only their own mailboxes. Treat a timeout as no message, not as a closed client.
- Model authorization-state changes and TDLib errors as application-owned events. Preserve unknown future authorization states rather than panicking or silently coercing them.
- Treat a close request and TDLib's `authorizationStateClosed` confirmation as separate lifecycle steps. Do not mark an account closed or release its routing state before confirmation; the preferred TDLib client-ID API automatically destroys the client after closure. Surface transport failures distinctly from clean shutdown.
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
