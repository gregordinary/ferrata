use std::string::ToString;
use std::vec;
use std::vec::Vec;

use super::*;

fn dependency(text: &str) -> Dependency {
    Dialect::V3_0
        .dependency(text)
        .unwrap_or_else(|error| panic!("{text:?}: {error}"))
}

fn version(text: &str) -> Version {
    Dialect::V3_0
        .version(text)
        .unwrap_or_else(|error| panic!("{text:?}: {error}"))
}

fn refused(text: &str) -> DependencyError {
    match Dialect::V3_0.dependency(text) {
        Ok(entry) => panic!("{text:?} was read as {entry}"),
        Err(error) => error,
    }
}

#[test]
fn a_name_alone_admits_every_version() {
    for text in ["lib", "so:libc.musl-x86_64.so.1", "cmd:[", "/bin/sh", "ünï"] {
        let entry = dependency(text);
        assert_eq!(entry.name(), text);
        assert!(!entry.is_conflict());
        assert_eq!(entry.identity(), None);
        assert!(entry.admits(&version("0")));
        assert!(entry.admits(&version("99.0-r0")));
    }
}

#[test]
fn a_name_is_everything_up_to_the_operator() {
    // apk.static 3.0.9 installs a package named each of these, requested by
    // its name.
    for text in [
        "we!rd", "a,b", "a*b", "A", ".x", "a/b", "x#y", "a;b", "a'b", "a\tb", "a\r",
    ] {
        assert_eq!(dependency(text).name(), text);
    }
    let entry = dependency("a:b>=1.0");
    assert_eq!(entry.name(), "a:b");
}

#[test]
fn the_operator_is_a_set_of_orderings() {
    // Every row is apk.static 3.0.9's answer for a request for `lib`, which
    // the repository offers at 2.5-r0 only.
    let offered = version("2.5-r0");
    for (text, admitted) in [
        ("lib=2.5-r0", true),
        ("lib=2.0", false),
        ("lib>2.0", true),
        ("lib<2.0", false),
        ("lib>=2.0", true),
        ("lib=>2.0", true),
        ("lib<=2.0", false),
        ("lib=<2.0", false),
        ("lib<=>2.0", true),
        ("lib==2.5-r0", true),
        ("lib>>2.0", true),
        ("lib<<3", true),
        ("lib~2.5", true),
        ("lib~2", true),
        ("lib=~2.5", true),
        ("lib~=2.5", true),
        ("lib~~2.5", true),
        ("lib~2.0", false),
        ("lib>~2.0", true),
        ("lib<~2.5", true),
        ("lib<~2.0", false),
    ] {
        assert_eq!(dependency(text).admits(&offered), admitted, "{text}");
    }
}

#[test]
fn a_prefix_operator_admits_what_begins_with_its_version() {
    for (text, candidate, admitted) in [
        ("lib~1.0", "1.0.5-r0", true),
        ("lib~1.0", "1.1.0-r0", false),
        ("lib~1.0", "1.0a-r0", true),
        ("lib~1.0", "1.0_rc1-r0", true),
        ("lib~1.0", "1.0-r5", true),
        ("lib~1.0-r1", "1.0-r1", true),
        ("lib~1.0-r1", "1.0-r2", false),
        ("lib~1.0_p", "1.0_p1-r0", true),
        ("lib~1.0_p1", "1.0_p2-r0", false),
        ("lib~1.0a", "1.0.1a-r0", false),
        ("lib~01.0", "1.0.5-r0", true),
        ("lib~1.1", "1.10-r0", false),
    ] {
        assert_eq!(
            dependency(text).admits(&version(candidate)),
            admitted,
            "{text} against {candidate}",
        );
    }
}

#[test]
fn a_conflict_is_marked_and_admits_as_its_constraint_does() {
    let entry = dependency("!lib>2");
    assert!(entry.is_conflict());
    assert_eq!(entry.name(), "lib");
    assert!(entry.admits(&version("2.5-r0")));
    assert!(!entry.admits(&version("1.0-r0")));
    // Only the first `!` marks a conflict.
    let entry = dependency("!!lib");
    assert!(entry.is_conflict());
    assert_eq!(entry.name(), "!lib");
}

#[test]
fn the_identity_operator_names_one_build() {
    for text in ["lib><Q1abc", "lib<>Q1abc", "lib><2.0"] {
        let entry = dependency(text);
        assert_eq!(entry.name(), "lib");
        assert!(entry.identity().is_some());
        assert!(!entry.admits(&version("2.0")), "{text}");
    }
    assert_eq!(dependency("lib><Q1abc").identity(), Some(&b"Q1abc"[..]));
    // Any other ordering added makes it an operator on versions again.
    let entry = dependency("lib<>~2.0");
    assert_eq!(entry.identity(), None);
    assert!(entry.admits(&version("2.0.1")));
}

#[test]
fn the_rendering_is_canonical_and_reads_back() {
    // apk.static 3.0.9 writes each of these into the world as shown, but for
    // the last two, which it writes as `lib2.0` and `lib?2.0`.
    for (text, rendered) in [
        ("lib", "lib"),
        ("lib=2.5-r0", "lib=2.5-r0"),
        ("lib==2.5-r0", "lib=2.5-r0"),
        ("lib=>2.0", "lib>=2.0"),
        ("lib>>2.0", "lib>2.0"),
        ("lib=<3", "lib<=3"),
        ("lib=~2.5", "lib~2.5"),
        ("lib~~2.5", "lib~2.5"),
        ("lib~<2.5", "lib<~2.5"),
        ("lib>~=2.0", "lib>~2.0"),
        ("!lib>2.0", "!lib>2.0"),
        ("lib<>Q1abc", "lib><Q1abc"),
        ("lib<=>2.0", "lib<>=2.0"),
        ("lib<>~2.0", "lib<>~2.0"),
    ] {
        let entry = dependency(text);
        assert_eq!(entry.to_string(), rendered, "{text}");
        assert_eq!(entry.to_bytes(), rendered.as_bytes(), "{text}");
        assert_eq!(dependency(rendered), entry, "{rendered}");
    }
}

/// Holds `text`, where it reads as an entry, to the writer: the entry's
/// bytes read back as the entry, write as the same bytes again, and are
/// what `Display` writes, but for its escapes. Says whether it read.
fn writes_back(text: &[u8]) -> bool {
    let Ok(entry) = Dialect::V3_0.dependency(text) else {
        return false;
    };
    let written = entry.to_bytes();
    let again = Dialect::V3_0
        .dependency(&written)
        .unwrap_or_else(|error| panic!("{text:?} is written {written:?}: {error}"));
    assert_eq!(again, entry, "{text:?} is written {written:?}");
    assert_eq!(again.to_bytes(), written, "{text:?}");
    assert_eq!(again.name(), entry.name(), "{text:?}");
    assert_eq!(again.identity(), entry.identity(), "{text:?}");
    assert_eq!(
        again.to_string(),
        crate::name::Text(&written).to_string(),
        "{text:?}"
    );
    if let Ok(plain) = core::str::from_utf8(&written)
        && !plain.contains('\\')
    {
        assert_eq!(entry.to_string(), plain, "{text:?}");
    }
    true
}

#[test]
fn every_short_entry_writes_back_as_itself() {
    // Every string of up to five bytes from an alphabet of the bytes that
    // shape an entry, and a backslash and a byte that is not UTF-8.
    let alphabet: &[u8] = b"a!<>=~1.\\\xff";
    let mut strings: Vec<Vec<u8>> = vec![Vec::new()];
    let mut read = 0;
    for length in 1..=5 {
        let shorter: Vec<Vec<u8>> = strings
            .iter()
            .filter(|string| string.len() == length - 1)
            .cloned()
            .collect();
        for string in shorter {
            for &byte in alphabet {
                let mut longer = string.clone();
                longer.push(byte);
                read += usize::from(writes_back(&longer));
                strings.push(longer);
            }
        }
    }
    assert!(read > 10_000, "{read} entries read");
}

#[test]
fn every_entry_made_at_random_writes_back_as_itself() {
    // Entries joined from the parts of real ones, and parts no entry
    // holds, with a small deterministic generator (SplitMix64).
    const PARTS: &[&[u8]] = &[
        b"lib",
        b"so:libc.musl-x86_64.so.1",
        b"cmd:",
        b"/bin/sh",
        b"!",
        b"\\",
        b"\xff",
        b"\xc3",
        b"\xc3\xa9",
        b"\xed\xa0\x80",
        b"\t",
        b"\r",
        b"@",
        b" ",
        b"<",
        b">",
        b"=",
        b"~",
        b"><",
        b"<>",
        b"1",
        b"1.0",
        b"2.5-r10",
        b"01.0_rc1",
        b"1.0b1",
        b"0~ff",
        b"Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        b"Q2",
        b"-r",
        b"_p",
    ];
    let mut state: u64 = 0x5eed;
    let mut next = |bound: usize| {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        usize::try_from((z ^ (z >> 31)) % u64::try_from(bound).unwrap()).unwrap()
    };
    let mut read = 0;
    for _ in 0..200_000 {
        let mut text = Vec::new();
        for _ in 0..=next(6) {
            text.extend_from_slice(PARTS[next(PARTS.len())]);
        }
        read += usize::from(writes_back(&text));
    }
    assert!(read > 20_000, "{read} entries read");
    // The entry a list holds where it begins with a space has no spelling.
    let empty = Dependency::on_empty_name(Dialect::V3_0);
    assert_eq!(empty.to_bytes(), b"");
    assert!(Dialect::V3_0.dependency(empty.to_bytes()).is_err());
    assert_eq!(empty.to_string(), "");
}

#[test]
fn what_is_not_an_entry_is_refused_where_it_goes_wrong() {
    for (text, kind, offset) in [
        ("", DependencyErrorKind::NoName, 0),
        ("!", DependencyErrorKind::NoName, 1),
        (">=1.0", DependencyErrorKind::NoName, 0),
        ("@edge", DependencyErrorKind::NoName, 0),
        (" lib", DependencyErrorKind::NoName, 0),
        ("lib ", DependencyErrorKind::Separator, 3),
        ("a b", DependencyErrorKind::Separator, 1),
        ("lib\nx", DependencyErrorKind::Separator, 3),
        ("lib@edge", DependencyErrorKind::Tag, 3),
        ("lib@x=2.5-r0", DependencyErrorKind::Tag, 3),
        ("lib>=", DependencyErrorKind::Truncated, 5),
        ("lib=", DependencyErrorKind::Truncated, 4),
        ("lib~", DependencyErrorKind::Truncated, 4),
        ("lib<", DependencyErrorKind::Truncated, 4),
        ("lib<>", DependencyErrorKind::Truncated, 5),
        ("lib><", DependencyErrorKind::Truncated, 5),
        ("lib><Q1 abc", DependencyErrorKind::Separator, 7),
        (
            "lib=abc",
            DependencyErrorKind::Version(VersionErrorKind::Unexpected),
            4,
        ),
        (
            "lib>2.0=",
            DependencyErrorKind::Version(VersionErrorKind::Unexpected),
            7,
        ),
        (
            "lib=2.5-r0@x",
            DependencyErrorKind::Version(VersionErrorKind::Unexpected),
            10,
        ),
        (
            "a:b=c",
            DependencyErrorKind::Version(VersionErrorKind::Unexpected),
            4,
        ),
        (
            "lib=1.0_x",
            DependencyErrorKind::Version(VersionErrorKind::UnknownSuffix),
            8,
        ),
    ] {
        let error = refused(text);
        assert_eq!((error.kind(), error.offset()), (kind, offset), "{text:?}");
    }
}

#[test]
fn an_error_says_what_and_where() {
    assert_eq!(
        refused("lib=1.0_x").to_string(),
        "not a dependency: an unknown suffix in the version at byte 8",
    );
}
