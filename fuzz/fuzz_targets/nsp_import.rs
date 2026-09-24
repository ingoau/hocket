//! Navidrome smart-playlist (`.nsp`) import. An imported filter must be
//! valid, and (unless it uses a field the server cannot evaluate) export and
//! re-import to the same filter.
#![no_main]

use hocket_core::filters::model::{validate_filter, FilterError, ServerCaps};
use hocket_core::filters::{from_nsp, to_nsp};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(filter) = from_nsp(text, "fallback") else {
        return;
    };
    validate_filter(&filter).expect("an imported filter validates");
    let caps = ServerCaps {
        sonic_attributes: true,
        native_api: true,
    };
    let exported = match to_nsp(&filter, caps) {
        Ok(s) => s,
        Err(FilterError::NotServerExpressible(_)) => return,
        Err(e) => panic!("an imported filter fails to export: {e:?}\n{filter:?}"),
    };
    let mut back = from_nsp(&exported, "other").expect("an exported filter imports");
    back.id = filter.id.clone();
    assert_eq!(back, filter, "export → import changed the filter:\n{exported}");
});
