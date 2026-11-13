//! Dependency entries over arbitrary input: reading never panics, an entry
//! written as bytes reads back as the entry it came from and is written the
//! same again, its `Display` is those bytes but for its escapes, and it
//! judges a version the same way after the round trip.
//!
//! The input is split at the first `\n`: an entry, then a version.

#![no_main]

use ferrata::{Dialect, Name};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let (text, version) = match input.iter().position(|&byte| byte == b'\n') {
        Some(at) => (&input[..at], &input[at + 1..]),
        None => (input, &b"1.0"[..]),
    };
    let Ok(entry) = Dialect::V3_0.dependency(text) else {
        return;
    };
    let written = entry.to_bytes();
    let version = Dialect::V3_0.version(version).ok();
    if let Some(version) = &version {
        entry.admits(version);
    }
    let again = Dialect::V3_0
        .dependency(&written)
        .unwrap_or_else(|error| panic!("{written:?} does not read back: {error}"));
    assert_eq!(again, entry);
    assert_eq!(again.name(), entry.name());
    assert_eq!(again.identity(), entry.identity());
    assert_eq!(again.to_bytes(), written);
    // `Display` escapes as a name's does, and only there.
    assert_eq!(
        entry.to_string(),
        Name::from(written.as_slice()).to_string()
    );
    if let Some(version) = &version {
        assert_eq!(entry.admits(version), again.admits(version));
    }
});
