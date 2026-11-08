# tdlib-ffi

A narrow Linux-only dynamic C ABI boundary for TDLib's preferred JSON client-ID interface.

The crate loads `libtdjson.so` at runtime and resolves only the pinned API:

- `td_create_client_id`
- `td_send`
- `td_receive`
- `td_execute`

It intentionally does not expose the deprecated `td_json_client_*` API. Returned TDLib strings are copied into owned Rust strings while a shared lock prevents another receive/execute call from invalidating the pointer. The application must still maintain exactly one process-wide receive dispatcher; this crate's lock is a memory-safety guard, not a substitute for that ownership rule.

The native library is built separately by `scripts/build-tdlib.sh`. Loading is explicit, so unit tests do not require TDLib to be installed.
