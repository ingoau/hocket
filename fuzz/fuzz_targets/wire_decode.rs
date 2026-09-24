//! Connect wire decoding: one inbound WebSocket text frame → `WireMessage`.
//!
//! Must never panic. Whatever decodes must re-encode and decode back to
//! itself, because a room relays what it receives.
#![no_main]

mod common;

use hocket_core::connect::wire::WireMessage;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Transports only hand text frames to `decode`, and tungstenite has
    // already checked they are UTF-8.
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(msg) = WireMessage::decode(text) else {
        return;
    };
    let _ = msg.msg.name();
    let again = msg.encode().expect("a decoded frame re-encodes");
    let back = WireMessage::decode(&again).expect("a re-encoded frame decodes");
    assert!(
        common::same_modulo_float_parsing(&back, &msg),
        "re-encoded frame decodes differently: {again}\n{back:?}\n{msg:?}"
    );
});
