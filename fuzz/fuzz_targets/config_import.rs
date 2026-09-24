//! Settings backup import: `settings::config::parse_document` on arbitrary
//! JSON (any version, with migrations). An imported document must export and
//! re-import unchanged.
#![no_main]

use hocket_core::settings::config::{parse_document, to_json, CONFIG_VERSION};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(doc) = parse_document(text) else {
        return;
    };
    assert_eq!(doc.version, CONFIG_VERSION);
    let json = to_json(&doc).expect("an imported document exports");
    let back = parse_document(&json).expect("an exported document imports");
    assert_eq!(back, doc, "export → import changed the document");
});
