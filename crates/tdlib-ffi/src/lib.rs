//! A small, audited dynamic-loading boundary for TDLib's preferred JSON C API.
//!
//! The function signatures match `td/telegram/td_json_client.h` at the
//! Telestra-pinned TDLib revision. The deprecated `td_json_client_*` API is
//! deliberately not loaded or exposed.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{CString, NulError};
use std::fmt;
use std::path::Path;

#[cfg(not(target_os = "linux"))]
compile_error!("tdlib-ffi currently supports Linux targets only");

#[allow(unsafe_code)]
mod dynamic {
    use super::{Path, TdJsonError, request_cstring};
    use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
    use std::fmt;
    use std::mem::{size_of, transmute_copy};
    use std::os::unix::ffi::OsStrExt;
    use std::ptr::NonNull;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    const RTLD_LOCAL: c_int = 0;
    const RTLD_NOW: c_int = 2;

    // TDLib's preferred JSON API has one process-wide receive stream. Keep
    // pointer-returning receive/execute calls serialized across all handles.
    static RESPONSE_LOCK: Mutex<()> = Mutex::new(());

    type CreateClientId = unsafe extern "C" fn() -> c_int;
    type SendRequest = unsafe extern "C" fn(c_int, *const c_char);
    type Receive = unsafe extern "C" fn(c_double) -> *const c_char;
    type Execute = unsafe extern "C" fn(*const c_char) -> *const c_char;

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }

    struct LibraryHandle(NonNull<c_void>);

    // SAFETY: the loader handle is immutable after construction. TDLib's
    // preferred API permits td_send from any thread; receive/execute calls
    // are serialized by the process-wide RESPONSE_LOCK.
    unsafe impl Send for LibraryHandle {}
    // SAFETY: see the Send implementation above; the handle is read-only.
    unsafe impl Sync for LibraryHandle {}

    impl Drop for LibraryHandle {
        fn drop(&mut self) {
            // SAFETY: this handle was returned by dlopen and is closed exactly once.
            unsafe {
                let _ = dlclose(self.0.as_ptr());
            }
        }
    }

    struct Api {
        _handle: LibraryHandle,
        create_client_id: CreateClientId,
        send: SendRequest,
        receive: Receive,
        execute: Execute,
    }

    /// A cloneable handle to one loaded TDLib shared library.
    #[derive(Clone)]
    pub struct TdJsonLibrary(Arc<Api>);

    impl fmt::Debug for TdJsonLibrary {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("TdJsonLibrary")
                .finish_non_exhaustive()
        }
    }

    impl TdJsonLibrary {
        /// Loads the preferred TDLib JSON API from a shared library.
        ///
        /// The library must export td_create_client_id, td_send, td_receive
        /// and td_execute with the signatures from the pinned TDLib header.
        pub fn load(path: impl AsRef<Path>) -> Result<Self, TdJsonError> {
            let path = CString::new(path.as_ref().as_os_str().as_bytes())
                .map_err(|_| TdJsonError::InvalidPath)?;
            // SAFETY: path is a valid NUL-terminated string and flags are valid
            // POSIX dlopen flags. The returned handle is owned below.
            let raw_handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
            let handle = NonNull::new(raw_handle)
                .map(LibraryHandle)
                .ok_or_else(|| TdJsonError::DynamicLoader(last_loader_error()))?;

            // SAFETY: each symbol is loaded with the exact function-pointer
            // signature declared by the pinned TDLib C header.
            let create_client_id =
                unsafe { load_symbol::<CreateClientId>(handle.0, c"td_create_client_id")? };
            // SAFETY: signature matches void td_send(int, const char *).
            let send = unsafe { load_symbol::<SendRequest>(handle.0, c"td_send")? };
            // SAFETY: signature matches const char *td_receive(double).
            let receive = unsafe { load_symbol::<Receive>(handle.0, c"td_receive")? };
            // SAFETY: signature matches const char *td_execute(const char *).
            let execute = unsafe { load_symbol::<Execute>(handle.0, c"td_execute")? };

            Ok(Self(Arc::new(Api {
                _handle: handle,
                create_client_id,
                send,
                receive,
                execute,
            })))
        }

        /// Allocates an opaque TDLib client identifier.
        pub fn create_client_id(&self) -> i32 {
            // SAFETY: the function pointer was resolved against the pinned C ABI.
            unsafe { (self.0.create_client_id)() }
        }

        /// Sends a JSON request to one TDLib client.
        pub fn send(&self, client_id: i32, request: &str) -> Result<(), TdJsonError> {
            let request = request_cstring(request)?;
            // SAFETY: request remains alive and NUL-terminated for the duration
            // of td_send; TDLib documents that this function is thread-safe.
            unsafe {
                (self.0.send)(client_id, request.as_ptr());
            }
            Ok(())
        }

        /// Receives and immediately copies one message from TDLib's shared stream.
        ///
        /// Call this only from the process-wide receive dispatcher. The global
        /// lock prevents concurrent receive/execute calls from invalidating the
        /// borrowed TDLib response before it has been copied into owned memory.
        pub fn receive(&self, timeout: Duration) -> Result<Option<String>, TdJsonError> {
            let _guard = RESPONSE_LOCK
                .lock()
                .map_err(|_| TdJsonError::ResponseLockPoisoned)?;
            // SAFETY: function signature matches the pinned C header. The returned
            // pointer is copied before releasing RESPONSE_LOCK or calling TDLib again.
            let response = unsafe { (self.0.receive)(timeout.as_secs_f64()) };
            if response.is_null() {
                Ok(None)
            } else {
                copy_response(response).map(Some)
            }
        }

        /// Executes a TDLib request documented as synchronously executable.
        pub fn execute(&self, request: &str) -> Result<String, TdJsonError> {
            let request = request_cstring(request)?;
            let _guard = RESPONSE_LOCK
                .lock()
                .map_err(|_| TdJsonError::ResponseLockPoisoned)?;
            // SAFETY: request is NUL-terminated and remains alive for the call.
            // The returned pointer is copied before releasing RESPONSE_LOCK.
            let response = unsafe { (self.0.execute)(request.as_ptr()) };
            copy_response(response)
        }
    }

    fn copy_response(response: *const c_char) -> Result<String, TdJsonError> {
        if response.is_null() {
            return Err(TdJsonError::NullResponse);
        }
        // SAFETY: TDLib returns a NUL-terminated UTF-8 JSON string whose lifetime
        // ends at the next td_receive or td_execute call. The caller holds the
        // process-wide response lock and copies it immediately.
        let response = unsafe { CStr::from_ptr(response) };
        response
            .to_str()
            .map(str::to_owned)
            .map_err(|_| TdJsonError::InvalidUtf8)
    }

    unsafe fn load_symbol<T: Copy>(
        handle: NonNull<c_void>,
        symbol: &'static CStr,
    ) -> Result<T, TdJsonError> {
        // SAFETY: handle is a live dlopen handle and symbol is NUL-terminated.
        let address = unsafe { dlsym(handle.as_ptr(), symbol.as_ptr()) };
        if address.is_null() {
            return Err(TdJsonError::DynamicLoader(last_loader_error()));
        }
        if size_of::<T>() != size_of::<*mut c_void>() {
            return Err(TdJsonError::InvalidSymbolAddress);
        }
        // SAFETY: callers instantiate T only with the function-pointer types
        // matching the official TDLib C declarations above.
        Ok(unsafe { transmute_copy(&address) })
    }

    fn last_loader_error() -> String {
        // SAFETY: dlerror returns either null or a NUL-terminated diagnostic
        // owned by the dynamic loader. Copy it before another loader call.
        let error = unsafe { dlerror() };
        if error.is_null() {
            "dynamic loader returned no diagnostic".to_owned()
        } else {
            // SAFETY: non-null dlerror result is a valid NUL-terminated string.
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned()
        }
    }

}

pub use dynamic::TdJsonLibrary;

/// Errors returned by the TDLib dynamic loader and C ABI boundary.
#[derive(Debug)]
pub enum TdJsonError {
    DynamicLoader(String),
    InvalidPath,
    InvalidSymbolAddress,
    InteriorNul(NulError),
    InvalidUtf8,
    NullResponse,
    ResponseLockPoisoned,
}

impl fmt::Display for TdJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DynamicLoader(message) => write!(formatter, "TDLib dynamic loader: {message}"),
            Self::InvalidPath => formatter.write_str("TDLib library path contains a NUL byte"),
            Self::InvalidSymbolAddress => {
                formatter.write_str("TDLib symbol address has an unsupported pointer size")
            }
            Self::InteriorNul(error) => {
                write!(formatter, "TDLib JSON request contains NUL: {error}")
            }
            Self::InvalidUtf8 => formatter.write_str("TDLib returned a non-UTF-8 response"),
            Self::NullResponse => formatter.write_str("TDLib returned a null response"),
            Self::ResponseLockPoisoned => formatter.write_str("TDLib response lock was poisoned"),
        }
    }
}

impl std::error::Error for TdJsonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InteriorNul(error) => Some(error),
            _ => None,
        }
    }
}

fn request_cstring(request: &str) -> Result<CString, TdJsonError> {
    CString::new(request).map_err(TdJsonError::InteriorNul)
}

#[cfg(test)]
mod tests {
    use super::{TdJsonError, TdJsonLibrary, request_cstring};

    #[test]
    fn reports_a_missing_shared_library() {
        assert!(matches!(
            TdJsonLibrary::load("/telestra/test/nonexistent/libtdjson.so"),
            Err(TdJsonError::DynamicLoader(_))
        ));
    }

    #[test]
    fn rejects_a_library_without_the_tdlib_json_abi() {
        assert!(matches!(
            TdJsonLibrary::load("libc.so.6"),
            Err(TdJsonError::DynamicLoader(_))
        ));
    }

    #[test]
    fn rejects_paths_with_embedded_nul_bytes() {
        assert!(matches!(
            TdJsonLibrary::load("libtdjson.so\0unexpected"),
            Err(TdJsonError::InvalidPath)
        ));
    }

    #[test]
    fn rejects_requests_with_embedded_nul_bytes_before_calling_tdlib() {
        assert!(matches!(
            request_cstring("{\"@type\":\"getMe\"}\0suffix"),
            Err(TdJsonError::InteriorNul(_))
        ));
    }
}
