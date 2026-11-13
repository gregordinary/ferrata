use std::string::ToString;

use super::*;

// Every reading below is apk.static 3.0.9's, measured by giving it an index
// holding the record and asking it to install the package.

const SHA1: &str = "Q1NH6x7ZVrTqn3VGktcofFNWa+fmQ=";
const SHA256: &str = "Q2JILcC8LuFWfNcf3tQlh1FSqvhFfIxTiwg0UnMK1OE3g=";

#[test]
fn an_identity_reads_as_either_digest() {
    let sha1 = Identity::read(SHA1).unwrap();
    assert_eq!(sha1.digest().len(), 20);
    assert_eq!(sha1.to_string(), SHA1);
    let sha256 = Identity::read(SHA256).unwrap();
    assert_eq!(sha256.digest().len(), 32);
    assert_eq!(sha256.to_string(), SHA256);
    assert_ne!(sha1, sha256);
    assert_eq!(std::format!("{sha1:?}"), std::format!("Identity({SHA1})"));
}

#[test]
fn an_identity_ignores_what_follows_its_digest() {
    for text in [
        "Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=x",
        "Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        "Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA= x",
    ] {
        let identity = Identity::read(text).unwrap();
        assert_eq!(identity.to_string(), "Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=");
    }
}

#[test]
fn an_identity_ignores_the_bits_its_padding_leaves_over() {
    let identity = Identity::read("Q1AAAAAAAAAAAAAAAAAAAAAAAAAAB=").unwrap();
    assert_eq!(identity.digest(), &[0; 20]);
    assert_eq!(identity.to_string(), "Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=");
}

#[test]
fn what_is_not_an_identity_does_not_read() {
    for text in [
        "",
        "xyz",
        "Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "Q1AAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "Q1AAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "Q1AAAAAAAAAAAAAAAAAAAAAAA!AAA=",
        "q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "Q2AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "Q3AAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "X1AAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "0123456789abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef01234567",
    ] {
        assert!(Identity::read(text).is_none(), "{text:?}");
    }
}

#[test]
fn identities_compare_by_digest() {
    let a = Identity::read("Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
    let b = Identity::read("Q1AAAAAAAAAAAAAAAAAAAAAAAAAAB=x").unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_provide_takes_the_text_after_any_operator_as_its_version() {
    let read = |text: &str| Provide::read(Dialect::V3_0, text.as_bytes()).unwrap();
    let bare = read("x");
    assert_eq!(bare.name(), "x");
    assert_eq!(bare.version(), None);
    for (text, version) in [
        ("x=1.0", "1.0"),
        ("x>=1.0", "1.0"),
        ("x~1.0", "1.0"),
        ("x><Q1abc", "Q1abc"),
        ("so:libtls.so.28=3.3.3p1", "3.3.3p1"),
    ] {
        let provide = read(text);
        assert_eq!(
            provide.version().map(Version::as_bytes),
            Some(version.as_bytes()),
            "{text}"
        );
    }
    assert!(!read("pc:libtls=3.3.3p1").version().unwrap().is_valid());
    assert_eq!(read("x>=1.0").to_string(), "x=1.0");
    assert!(Provide::read(Dialect::V3_0, b"x=").is_none());
    assert!(Provide::read(Dialect::V3_0, b"x>=").is_none());
}

#[test]
fn flaws_say_what_they_are() {
    assert_eq!(Flaw::UnknownField(b'B').to_string(), "an unknown field 'B'");
    assert_eq!(
        Flaw::UnknownField(0xff).to_string(),
        "an unknown field '\\xff'"
    );
    assert_eq!(
        Flaw::Provides.to_string(),
        "a provided name with an operator and no version"
    );
}

// Measured by asking apk.static 3.0.9 to install each package: those that
// conflict with themselves it refuses, naming the package twice.
#[test]
fn a_package_providing_a_name_twice_conflicts_with_itself() {
    let read = |provides: &str| {
        let text = std::format!("C:{SHA1}\nP:a\nV:1\np:{provides}\n");
        crate::index::read(Dialect::V3_0, &text)
            .next()
            .unwrap()
            .unwrap()
            .into_record()
    };
    for provides in [
        "x x=1", "x=1 x", "x=1 x=2", "x=1 x=1", "x x x=1", "a", "a=1", "a=2",
    ] {
        let record = read(provides);
        assert!(record.conflicts_with_itself(), "{provides}");
        assert!(!record.is_installable(), "{provides}");
    }
    for provides in ["x", "x x", "x=1 y=1", "x y"] {
        let record = read(provides);
        assert!(!record.conflicts_with_itself(), "{provides}");
        assert!(record.is_installable(), "{provides}");
    }
}

/// The one record `text` holds, which states no identity.
fn record(text: &str) -> Record {
    let text = std::format!("C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\n{text}\n");
    crate::index::read(Dialect::V3_0, &text)
        .next()
        .unwrap()
        .unwrap()
        .into_record()
}

fn answers(record: &Record, entry: &str) -> bool {
    record.answers(&Dialect::V3_0.dependency(entry).unwrap())
}

#[test]
fn a_name_stated_twice_answers_at_its_first_mention() {
    let own = record("P:a\nV:1\np:a=2");
    assert!(answers(&own, "a=1"));
    assert!(!answers(&own, "a=2"));
    let twice = record("P:p\nV:1\np:x=1 x=2");
    assert!(answers(&twice, "x=1"));
    assert!(!answers(&twice, "x=2"));
    let bare_first = record("P:p\nV:1\np:x x=2");
    assert!(answers(&bare_first, "x"));
    assert!(answers(&bare_first, "x<2"));
    assert!(!answers(&bare_first, "x>=2"));
}

#[test]
fn an_identity_is_answered_by_the_package_carrying_the_name() {
    let record = record("P:a\nV:1\np:b=1");
    assert!(answers(&record, "a><Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA="));
    assert!(!answers(&record, "a><Q1BAAAAAAAAAAAAAAAAAAAAAAAAAA="));
    assert!(!answers(&record, "b><Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA="));
    assert!(!answers(&record, "a><nonsense"));
}
