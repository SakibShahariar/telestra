//! Native ABI smoke test. The regular Rust CI skips this when TDLib is not
//! installed; the dedicated native-build workflow always supplies the library.

use std::env;

use tdlib_ffi::TdJsonLibrary;

#[test]
fn loads_pinned_tdlib_and_executes_a_synchronous_request() {
    let Ok(path) = env::var("TELESTRA_TDLIB_LIBRARY") else {
        eprintln!("skipping native TDLib smoke test: TELESTRA_TDLIB_LIBRARY is unset");
        return;
    };

    let library = TdJsonLibrary::load(path).expect("load the freshly built TDLib library");
    let response = library
        .execute(r#"{"@type":"setLogVerbosityLevel","new_verbosity_level":0}"#)
        .expect("execute a synchronous TDLib request");

    assert!(
        response.contains(r#""@type":"ok""#),
        "TDLib returned an unexpected response: {response}"
    );

    let client_id = library.create_client_id();
    assert!(client_id >= 0, "TDLib returned an invalid client identifier");
}
