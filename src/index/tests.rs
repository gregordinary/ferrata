use std::format;
use std::string::{String, ToString};
use std::vec::Vec;

use super::*;
use crate::Name;
use crate::dependency::DependencyErrorKind;
use crate::version::VersionErrorKind;

// Every reading below is apk.static 3.0.9's, measured by giving it an index
// holding the records and asking it to install the packages.

const A: &str = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const B: &str = "C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=";

/// Every record the text holds, or the first error.
fn records(text: &str) -> Result<Vec<Record>, ReadError> {
    read(Dialect::V3_0, text)
        .map(|item| item.map(Located::into_record))
        .collect()
}

fn record(text: &str) -> Record {
    let mut records = records(text).unwrap_or_else(|error| panic!("{text:?}: {error}"));
    assert_eq!(records.len(), 1, "{text:?}");
    records.remove(0)
}

/// The records read before an error, and the error.
fn until_error(text: &str) -> (Vec<String>, ReadError) {
    let mut names = Vec::new();
    for item in read(Dialect::V3_0, text) {
        match item {
            Ok(located) => names.push(located.record().name().to_string()),
            Err(error) => return (names, error),
        }
    }
    panic!("{text:?} read without an error");
}

fn names(dependencies: &[Dependency]) -> Vec<&Name> {
    dependencies.iter().map(Dependency::name).collect()
}

#[test]
fn a_record_reads_its_fields() {
    let text = format!(
        "{A}\nP:7zip\nV:26.01-r0\nA:x86_64\nS:917978\nT:File archiver\no:7zip\n\
         k:100\nD:so:libc.musl-x86_64.so.1 !p7zip<17\np:cmd:7z=26.01-r0 7z\ni:7zip-libs docs\n"
    );
    let record = record(&text);
    assert_eq!(record.name(), "7zip");
    assert_eq!(record.version().to_string(), "26.01-r0");
    assert_eq!(record.identity().to_string(), &A[2..]);
    assert_eq!(record.architecture(), Some(&b"x86_64"[..]));
    assert_eq!(record.origin(), Some(&b"7zip"[..]));
    assert_eq!(record.priority(), 100);
    assert_eq!(
        names(record.depends()),
        ["so:libc.musl-x86_64.so.1", "p7zip"]
    );
    assert!(record.depends()[1].is_conflict());
    let provides: Vec<String> = record.provides().iter().map(Provide::to_string).collect();
    assert_eq!(provides, ["cmd:7z=26.01-r0", "7z"]);
    assert_eq!(names(record.install_if()), ["7zip-libs", "docs"]);
    assert_eq!(record.field(b'T'), Some(&b"File archiver"[..]));
    assert_eq!(record.field(b'S'), Some(&b"917978"[..]));
    assert_eq!(record.field(b'm'), None);
    assert_eq!(record.fields().count(), 11);
    assert!(record.flaws().is_empty());
    assert!(record.is_installable());
    assert_eq!(record.to_string(), text);
}

#[test]
fn records_are_separated_by_blank_lines_and_located() {
    let text = format!("\n{A}\nP:one\nV:1\n\n\n{B}\nP:two\nV:2");
    let located: Vec<Located> = read(Dialect::V3_0, &text).map(Result::unwrap).collect();
    assert_eq!(located.len(), 2);
    assert_eq!(located[0].line(), 2);
    assert_eq!(located[1].line(), 7);
    for item in &located {
        let span = &text[item.span()];
        assert!(span.starts_with("C:"), "{span:?}");
        assert!(!span.contains("\n\n"), "{span:?}");
        // Read alone, the span is the record again.
        assert_eq!(record(span), *item.record());
    }
    // The last record has no line feed, and its span ends with the text.
    // Written back, its last line gains one.
    assert_eq!(located[1].span().end, text.len());
    assert_eq!(located[0].record().to_string(), &text[located[0].span()]);
    assert_eq!(
        located[1].record().to_string(),
        format!("{}\n", &text[located[1].span()])
    );
}

#[test]
fn a_repeated_field_replaces_or_adds_to_the_one_before() {
    let record = record(&format!(
        "{A}\nP:first\nP:second\nV:1\nV:2\nk:5\nk:7\nD:a\nD:b\np:x\np:y\ni:c\ni:d\n"
    ));
    assert_eq!(record.name(), "second");
    assert_eq!(record.version().to_string(), "2");
    assert_eq!(record.priority(), 7);
    assert_eq!(names(record.depends()), ["a", "b"]);
    assert_eq!(record.provides().len(), 2);
    assert_eq!(names(record.install_if()), ["c", "d"]);
    assert_eq!(record.field(b'P'), Some(&b"second"[..]));
}

#[test]
fn a_carriage_return_is_part_of_its_value() {
    let record = record(&format!("{A}\nP:app\r\nV:1.0-r0\nD:a\r\n"));
    assert_eq!(record.name(), "app\r");
    assert_eq!(names(record.depends()), ["a\r"]);
}

#[test]
fn list_entries_are_separated_by_spaces_alone() {
    let depends = |value: &str| {
        let record = record(&format!("{A}\nP:app\nV:1\nD:{value}\n"));
        names(record.depends())
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(depends("a  b"), ["a", "b"]);
    assert_eq!(depends("a b "), ["a", "b"]);
    assert_eq!(depends("a\tb"), ["a\tb"]);
    assert_eq!(depends(""), [] as [&str; 0]);
    // A list that begins with a space begins with an empty entry: a
    // dependency on the empty name, which nothing provides.
    assert_eq!(depends(" a b"), ["", "a", "b"]);
    assert_eq!(depends("  a"), ["", "a"]);
}

#[test]
fn a_version_outside_the_grammar_is_kept() {
    for value in ["1.0b1-r0", "0..0", ""] {
        let record = record(&format!("{A}\nP:app\nV:{value}\n"));
        assert_eq!(record.version().to_string(), value);
        assert!(!record.version().is_valid());
        assert!(record.is_installable());
    }
}

#[test]
fn fields_without_a_name_carry_into_the_next_record() {
    // apk.static 3.0.9 installs `after`, with `dep`, for a request for `bad`.
    let text = format!(
        "{A}\nV:1.0-r0\np:bad=1\nD:dep\n\n{B}\nP:after\nV:2.0-r0\n\nC:Q1DDDDDDDDDDDDDDDDDDDDDDDDDDD=\nP:dep\nV:1\n"
    );
    let located: Vec<Located> = read(Dialect::V3_0, &text).map(Result::unwrap).collect();
    assert_eq!(located.len(), 2);
    let after = located[0].record();
    assert_eq!(after.name(), "after");
    assert_eq!(after.version().to_string(), "2.0-r0");
    assert_eq!(Some(*after.identity()), Identity::read(&B[2..]));
    assert_eq!(after.provides()[0].to_string(), "bad=1");
    assert_eq!(names(after.depends()), ["dep"]);
    // The span covers both parts, and the record writes back without the
    // blank line between them.
    assert_eq!(located[0].span().start, 0);
    assert_eq!(located[0].line(), 1);
    let span = &text[located[0].span()];
    assert_eq!(after.to_string(), span.replace("\n\n", "\n"));
}

#[test]
fn fields_without_a_name_at_the_end_are_dropped() {
    let kept = records(&format!("{A}\nP:one\nV:1\n\n{B}\nV:1.0-r0\np:bad\n\n")).unwrap();
    assert_eq!(kept.len(), 1);
    assert!(records(&format!("{A}\nV:1\n\n\n")).unwrap().is_empty());
}

#[test]
fn an_empty_name_is_a_name() {
    let text = format!("{A}\nP:\nV:1.0-r0\np:bad=1\n\n{B}\nP:after\nV:2.0-r0\n");
    let read = records(&text).unwrap();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].name(), "");
}

#[test]
fn the_identity_is_judged_by_the_last_c_field() {
    assert!(records(&format!("C:xyz\n{A}\nP:app\nV:1\n")).is_ok());
    assert!(records(&format!("C:xyz\np:x\n\n{A}\nP:app\nV:1\n")).is_ok());
    let (_, error) = until_error(&format!("{A}\nP:one\nV:1\n\n{B}\nC:xyz\nP:app\nV:1\n"));
    assert_eq!((error.kind(), error.line()), (ReadErrorKind::Identity, 6));
}

#[test]
fn reading_stops_where_apk_stops() {
    for (text, kind, line) in [
        (
            format!("{A}\nP:one\nV:1\n\nP:two\n{B}\n\n"),
            ReadErrorKind::NoVersion,
            6,
        ),
        (
            format!("{A}\nP:one\nV:1\n\nP:two\nV:1\n"),
            ReadErrorKind::NoIdentity,
            6,
        ),
        (
            format!("{A}\nP:one\nV:1\n\nC:xyz\nP:two\nV:1\n"),
            ReadErrorKind::Identity,
            5,
        ),
        (
            format!("{A}\nP:one\nV:1\n\n{B}\nP:two\ngarbage\nV:1\n"),
            ReadErrorKind::NotAField,
            7,
        ),
        (
            format!("{A}\nP:one\nV:1\n\n{B}\nP:two\nPP:x\nV:1\n"),
            ReadErrorKind::NotAField,
            7,
        ),
        (
            format!("{A}\nP:one\nV:1\n\n{B}\nP:two\nü:x\nV:1\n"),
            ReadErrorKind::NotAField,
            7,
        ),
        (
            format!("{A}\nP:one\nV:1\n\n{B}\nP:two\n:x\nV:1\n"),
            ReadErrorKind::NotAField,
            7,
        ),
    ] {
        let (read, error) = until_error(&format!("{text}\n{A}\nP:after\nV:1\n"));
        assert_eq!(read, ["one"], "{text:?}");
        assert_eq!(
            (error.kind(), error.line(), error.record_line()),
            (kind, line, 5),
            "{text:?}"
        );
    }
    let mut records = read(Dialect::V3_0, "garbage\n");
    assert!(records.next().unwrap().is_err());
    assert!(records.next().is_none());
}

#[test]
fn an_error_says_what_and_where() {
    let (_, error) = until_error(&format!("{A}\nP:one\nV:1\n\n{B}\nP:two\ngarbage\n"));
    assert_eq!(
        error.to_string(),
        "line 7, in the record from line 5: a line that is not a field"
    );
}

#[test]
fn a_letter_apk_does_not_know_makes_a_flaw() {
    for letter in ['B', 'E', 'G', 'X', 'Y', '0', '1', '_', '#'] {
        let record = record(&format!("{A}\nP:app\nV:1\n{letter}:1\n"));
        assert_eq!(
            record.flaws(),
            [Flaw::UnknownField(letter as u8)],
            "{letter}"
        );
        assert!(!record.is_installable(), "{letter}");
    }
    for letter in [
        'b', 'd', 'e', 'h', 'j', 'l', 'n', 'u', 'v', 'w', 'x', 'y', 'z',
    ] {
        let record = record(&format!("{A}\nP:app\nV:1\n{letter}:1\n"));
        assert_eq!(
            record.flaws(),
            [Flaw::UnknownField(letter as u8)],
            "{letter}"
        );
        assert!(record.is_installable(), "{letter}");
    }
    for letter in [
        'a', 'c', 'f', 'g', 'i', 'k', 'm', 'o', 'p', 'q', 'r', 's', 't', 'F', 'I', 'L', 'M', 'R',
        'S', 'T', 'U', 'Z',
    ] {
        let record = record(&format!("{A}\nP:app\nV:1\n{letter}:1\n"));
        assert!(record.flaws().is_empty(), "{letter}");
    }
}

#[test]
fn a_dependency_that_does_not_read_makes_the_record_uninstallable() {
    let record = record(&format!("{A}\nP:app\nV:1\nD:a b=1.0b1 c\n"));
    assert_eq!(names(record.depends()), ["a", "c"]);
    let [Flaw::Depends(error)] = record.flaws() else {
        panic!("{:?}", record.flaws());
    };
    assert_eq!(
        error.kind(),
        DependencyErrorKind::Version(VersionErrorKind::Unexpected)
    );
    assert!(!record.is_installable());
}

#[test]
fn a_provide_that_does_not_read_ends_its_field() {
    let stopped = record(&format!("{A}\nP:prov\nV:1\np:x=1 w= v=1\np:y=1\n"));
    let provided: Vec<&Name> = stopped.provides().iter().map(Provide::name).collect();
    assert_eq!(provided, ["x", "y"]);
    assert_eq!(stopped.flaws(), [Flaw::Provides]);
    assert!(stopped.is_installable());
}

#[test]
fn an_install_if_that_does_not_read_empties_the_condition_so_far() {
    for (fields, condition) in [
        ("i:docs base=1.0b1-r0\n", &[] as &[&str]),
        ("i:docs\ni:docs=1.0b1\n", &[]),
        ("i:docs=1.0b1\ni:docs\n", &["docs"]),
        ("i:docs docs=1.0b1\ni:docs\n", &["docs"]),
        ("i:docs\ni:docs2\n", &["docs", "docs2"]),
    ] {
        let read = record(&format!("{A}\nP:extra\nV:1\n{fields}"));
        assert_eq!(names(read.install_if()), condition, "{fields:?}");
        assert!(read.is_installable());
    }
}

#[test]
fn a_priority_keeps_its_leading_digits_modulo_two_to_the_sixteen() {
    for (value, priority) in [
        ("20", 20),
        ("020", 20),
        ("20 ", 20),
        ("20x", 20),
        ("1e2", 1),
        ("", 0),
        ("abc", 0),
        ("-5", 0),
        ("+20", 0),
        ("256", 256),
        ("65535", 65535),
        ("65536", 0),
        ("65546", 10),
        ("131077", 5),
        ("2147483648", 0),
        ("4294967295", 65535),
        ("4294967306", 10),
        ("99999999999", 59391),
        ("18446744073709551615", 65535),
        ("18446744073709551616", 65535),
        ("99999999999999999999999", 65535),
    ] {
        let record = record(&format!("{A}\nP:p\nV:1\nk:{value}\n"));
        assert_eq!(record.priority(), priority, "k:{value}");
    }
}

#[test]
fn an_installed_database_passes_over_its_file_lines() {
    let text = format!(
        "{A}\nP:alpine-baselayout\nV:3.7.2-r0\nA:x86_64\nD:/bin/sh\nq:1000\n\
         F:etc\nR:motd\nZ:Q1SLkS9hBidUbPwwrw+XR0Whv3ww8=\nF:etc/crontabs\nM:0:0:700\n\
         R:root\na:0:0:600\nZ:Q1vfk1apUWI4yLJGhhNRd0kJixfvY=\n\n\
         {B}\nP:musl\nV:1.2.5-r10\nr:musl-dev\n\n"
    );
    let located: Vec<Located> = read_installed(Dialect::V3_0, &text)
        .map(Result::unwrap)
        .collect();
    assert_eq!(located.len(), 2);
    let record = located[0].record();
    let letters: Vec<u8> = record.fields().map(|(letter, _)| letter).collect();
    assert_eq!(letters, b"CPVADq");
    assert!(record.flaws().is_empty());
    // The span covers the file lines, so the database splits where the
    // reader splits it.
    assert!(text[located[0].span()].ends_with("Z:Q1vfk1apUWI4yLJGhhNRd0kJixfvY=\n"));
    assert_eq!(located[1].record().field(b'r'), Some(&b"musl-dev"[..]));
    // Read as an index, the same text keeps every line.
    let record = &records(&text).unwrap()[0];
    assert_eq!(record.fields().count(), 14);
    assert_eq!(record.to_string(), &text[located[0].span()]);
}

/// The fields of a record's text, one to a line, as `Record::from_fields`
/// takes them.
fn fields_of(text: &[u8]) -> Vec<(u8, &[u8])> {
    text.split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| (line[0], &line[2..]))
        .collect()
}

#[test]
fn a_record_made_of_its_fields_is_the_record_read() {
    for text in [
        format!(
            "{A}\nP:7zip\nV:26.01-r0\nA:x86_64\nD:so:libc.musl-x86_64.so.1\np:cmd:7z=26.01-r0\n"
        ),
        // Fields repeated, in another order, holding what does not read, or
        // a carriage return, letters apk does not know, and a version
        // outside the grammar.
        format!("P:a\nV:1\n{A}\nP:b\nV:1.0b1-r0\nk:65546\nD:x  y=z\nD:\nI:5\r\n"),
        format!("{A}\nP:\nV:1\np:x=1 y= z\ni:a i:b=\ni:c\nX:1\nx:1\no:src\nF:usr\nR:f\n"),
    ] {
        let read = records(&text).unwrap().remove(0);
        let made = Record::from_fields(Dialect::V3_0, fields_of(text.as_bytes())).unwrap();
        assert_eq!(made, read, "{text:?}");
        let again = Record::from_fields(Dialect::V3_0, read.fields()).unwrap();
        assert_eq!(again, read, "{text:?}");
    }
}

#[test]
fn fields_that_make_no_record_are_refused_as_the_reader_refuses_them() {
    let a = &A.as_bytes()[2..];
    let error = |fields: &[(u8, &[u8])]| {
        let error = Record::from_fields(Dialect::V3_0, fields.iter().copied()).unwrap_err();
        (error.kind(), error.line(), error.record_line())
    };
    assert_eq!(
        error(&[(b'C', a), (b'P', b"one")]),
        (ReadErrorKind::NoVersion, 2, 1)
    );
    assert_eq!(
        error(&[(b'P', b"one"), (b'V', b"1")]),
        (ReadErrorKind::NoIdentity, 2, 1)
    );
    assert_eq!(
        error(&[(b'C', b"xyz"), (b'P', b"one"), (b'V', b"1")]),
        (ReadErrorKind::Identity, 1, 1)
    );
    // The reader passes over fields that no `P:` ends.
    assert_eq!(
        error(&[(b'C', a), (b'V', b"1")]),
        (ReadErrorKind::NoName, 2, 1)
    );
    assert_eq!(error(&[]), (ReadErrorKind::NoName, 1, 1));
    assert!(records(&format!("{A}\nV:1\n")).unwrap().is_empty());
    // A line feed would end the field's line.
    assert_eq!(
        error(&[(b'C', a), (b'P', b"one\nV:1")]),
        (ReadErrorKind::NotAField, 2, 1)
    );
    assert_eq!(
        error(&[(b'C', a), (b'\n', b"x"), (b'P', b"one"), (b'V', b"1")]),
        (ReadErrorKind::NotAField, 2, 1)
    );
}

// apk.static 3.0.9 reads every field as bytes, and holds none to UTF-8: each
// reading below is its answer to an index or a database holding bytes that
// are not UTF-8.

/// The records of `text`, read as bytes.
fn records_of(text: &[u8]) -> Vec<Record> {
    read(Dialect::V3_0, text)
        .map(|item| item.map(Located::into_record))
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn a_name_that_is_not_utf8_is_read_byte_for_byte() {
    // apk installs `a\xff` when asked for it, and for `b`, which needs it.
    let text = b"C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\xff\nV:1.0-r0\nT:caf\xe9\n\n\
                 C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:b\nV:1.0-r0\nD:a\xff x\xed\xa0\x80>=1\n\
                 p:y\xc3=1 z\xc0\xaf\ni:a\xff\n";
    let records = records_of(text);
    assert_eq!(records[0].name(), b"a\xff");
    assert_eq!(records[0].name().to_str(), None);
    assert_eq!(records[0].field(b'T'), Some(&b"caf\xe9"[..]));
    assert!(records[0].is_installable());
    let b = &records[1];
    assert_eq!(names(b.depends()), [&b"a\xff"[..], b"x\xed\xa0\x80"]);
    let provided: Vec<&Name> = b.provides().iter().map(Provide::name).collect();
    assert_eq!(provided, [&b"y\xc3"[..], b"z\xc0\xaf"]);
    assert_eq!(names(b.install_if()), [&b"a\xff"[..]]);
    assert!(b.flaws().is_empty());
    // Each record writes back as the bytes it was read from, the last
    // gaining a line feed.
    for located in read(Dialect::V3_0, text).map(Result::unwrap) {
        let span = &text[located.span()];
        let written = located.record().as_bytes();
        assert_eq!(
            written.strip_suffix(b"\n"),
            span.strip_suffix(b"\n").or(Some(span))
        );
    }
}

#[test]
fn a_version_holding_a_byte_that_is_not_utf8_is_kept_and_not_valid() {
    // apk installs `a` at `1.0\xff-r0`; of it and `1.0-r0` it takes the
    // second, listed first or not, and of `1.1\xff` and `1.0-r0` the first:
    // the version reads as far as the byte.
    let records = records_of(
        b"C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\nV:1.0\xff-r0\n\n\
          C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:a\nV:1.1\xff\n",
    );
    let version = records[0].version();
    assert_eq!(version.as_bytes(), b"1.0\xff-r0");
    assert!(!version.is_valid());
    assert_eq!(version.to_str(), None);
    assert_eq!(version.to_string(), r"1.0\xff-r0");
    let released = Dialect::V3_0.version("1.0-r0").unwrap();
    assert!(version < &released);
    assert!(records[1].version() > &released);
    assert!(records[0].is_installable());
}

#[test]
fn a_backslash_is_a_byte_like_any_other() {
    // apk installs `a\xff`, the four bytes, and `a` followed by the byte
    // 0xFF side by side, each when asked for it or through a dependency; and
    // a package at `1.0\xff`, the four bytes, which it ranks as `1.0`
    // followed by the byte 0xFF, taking whichever is listed first.
    let records = records_of(
        b"C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\\xff\nV:1.0\\xff\n\n\
          C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:a\xff\nV:1.0\xff\n\n\
          C:Q1CCCCCCCCCCCCCCCCCCCCCCCCCCC=\nP:d\nV:1\nD:a\\xff\n",
    );
    let (literal, byte) = (&records[0], &records[1]);
    assert_eq!(literal.name(), br"a\xff");
    assert_eq!(byte.name(), b"a\xff");
    assert_ne!(literal.name(), byte.name());
    assert_eq!(names(records[2].depends()), [&br"a\xff"[..]]);
    assert!(!literal.version().is_valid());
    assert_eq!(literal.version(), byte.version());
    // Written as text, the two stay apart, as they do in `Debug`.
    assert_eq!(literal.name().to_string(), r"a\\xff");
    assert_eq!(byte.name().to_string(), r"a\xff");
    assert_eq!(literal.version().to_string(), r"1.0\\xff");
    assert_eq!(byte.version().to_string(), r"1.0\xff");
    assert_eq!(format!("{:?}", literal.name()), r#""a\\xff""#);
    assert_eq!(format!("{:?}", byte.name()), r#""a\xff""#);
}

#[test]
fn an_entry_whose_version_holds_a_byte_that_is_not_utf8_does_not_read() {
    // apk cannot install `b`: the entry's version is not one.
    let record = record_of(b"C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:b\nV:1\nD:a>=1.0\xff\n");
    let [Flaw::Depends(error)] = record.flaws() else {
        panic!("{:?}", record.flaws());
    };
    assert_eq!(
        error.kind(),
        DependencyErrorKind::Version(VersionErrorKind::Unexpected)
    );
    assert!(!record.is_installable());
    // A priority is the digits it begins with.
    let record = record_of(b"C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:p\nV:1\nk:10\xff\n");
    assert_eq!(record.priority(), 10);
}

fn record_of(text: &[u8]) -> Record {
    let mut records = records_of(text);
    assert_eq!(records.len(), 1);
    records.remove(0)
}

#[test]
fn a_letter_that_is_not_ascii_is_a_field_apk_does_not_know() {
    // apk cannot install a package with a field `\xff:` or `\xc3:`; at
    // `é:`, two bytes before the colon, it stops reading the index.
    for letter in [0xff, 0xc3, 0x80] {
        let text = [
            &b"C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\nV:1\n"[..],
            &[letter],
            b":value\n",
        ]
        .concat();
        let record = record_of(&text);
        assert_eq!(record.flaws(), [Flaw::UnknownField(letter)]);
        assert!(!record.is_installable());
        assert_eq!(record.field(letter), Some(&b"value"[..]));
    }
    let text = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\nV:1\né:value\n";
    let (read, error) = until_error(text);
    assert!(read.is_empty());
    assert_eq!(error.kind(), ReadErrorKind::NotAField);
    assert_eq!(error.line(), 4);
}

#[test]
fn an_installed_database_s_file_lines_may_hold_any_bytes() {
    let first: &[u8] = b"C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\xff\nV:1.0-r0\nF:usr/sh\xe9\n\
                         R:file\xff\xfe\na:0:0:755\nZ:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\n";
    let database = [first, b"\nC:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:b\nV:1\n"].concat();
    let located: Vec<Located> = read_installed(Dialect::V3_0, &database)
        .map(Result::unwrap)
        .collect();
    assert_eq!(located.len(), 2);
    assert_eq!(&database[located[0].span()], first);
    let record = located[0].record();
    assert_eq!(record.name(), b"a\xff");
    let letters: Vec<u8> = record.fields().map(|(letter, _)| letter).collect();
    assert_eq!(letters, b"CPV");
    assert_eq!(located[1].record().name(), "b");
}
