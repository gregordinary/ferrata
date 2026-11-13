//! Versions over arbitrary input: reading never panics, and the ordering is
//! total and consistent with equality, over versions the grammar allows and
//! over those a package record holds that it does not, bytes that are not
//! UTF-8 included.
//!
//! The input is split at `\n`, and its first three strings are kept. Each is
//! read as a record's version ([`Dialect::recorded_version`]), which keeps
//! any bytes and must be the version a record stating it carries, and every
//! pair and triple is checked. A string the grammar allows must read the
//! same strictly.

#![no_main]

use ferrata::Dialect;
use ferrata::version::Version;
use libfuzzer_sys::fuzz_target;

/// `text` as a package record holds it. The record names its package: a
/// record without `P:` is not one, and reads as nothing.
fn record_version(text: &[u8]) -> Version {
    let record = [b"C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:p\nV:", text, b"\n"].concat();
    let located = ferrata::index::read(Dialect::V3_0, &record)
        .next()
        .unwrap()
        .unwrap();
    located.record().version().clone()
}

fuzz_target!(|input: &[u8]| {
    let texts: Vec<&[u8]> = input.split(|&byte| byte == b'\n').take(3).collect();
    let versions: Vec<Version> = texts
        .iter()
        .map(|text| Dialect::V3_0.recorded_version(text))
        .collect();
    for (text, version) in texts.iter().copied().zip(&versions) {
        assert_eq!(version.as_bytes(), text);
        let recorded = record_version(text);
        assert_eq!(recorded.as_bytes(), text);
        assert_eq!(recorded.is_valid(), version.is_valid(), "{text:?}");
        assert!(recorded == *version, "{text:?}");
        match Dialect::V3_0.version(text) {
            Ok(strict) => {
                assert!(version.is_valid(), "{text:?}");
                assert!(strict == *version, "{text:?}");
                assert_eq!(version.to_str().map(str::as_bytes), Some(text));
            }
            Err(_) => assert!(!version.is_valid(), "{text:?}"),
        }
    }
    for a in &versions {
        assert!(a == a);
        for b in &versions {
            assert_eq!(a.cmp(b), b.cmp(a).reverse(), "{a} against {b}");
            assert_eq!(a == b, a.cmp(b).is_eq(), "{a} against {b}");
            for c in &versions {
                if a <= b && b <= c {
                    assert!(a <= c, "{a} <= {b} <= {c}");
                }
            }
        }
    }
});
