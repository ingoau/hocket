//! `api::Command` / `api::Query` JSON as the bindings' `dispatch_json` /
//! `query_json` receive it (deserialisation only). Whatever parses must
//! serialise and parse back to itself.
#![no_main]

mod common;

use hocket_core::api::{Command, Query};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(cmd) = serde_json::from_slice::<Command>(data) {
        let s = serde_json::to_string(&cmd).expect("a command serialises");
        let back: Command = serde_json::from_str(&s).expect("a serialised command parses");
        assert!(common::same_modulo_float_parsing(&back, &cmd), "{s}");
    }
    if let Ok(q) = serde_json::from_slice::<Query>(data) {
        let s = serde_json::to_string(&q).expect("a query serialises");
        let back: Query = serde_json::from_str(&s).expect("a serialised query parses");
        assert!(common::same_modulo_float_parsing(&back, &q), "{s}");
    }
});
