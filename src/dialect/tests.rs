use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Dialect, DialectErrorKind, upstream};
use crate::index;
use crate::record::Record;

/// The records of an index stating each `(name, version)`, in order.
fn records(packages: &[(&str, &str)]) -> Vec<Record> {
    let text: String = packages
        .iter()
        .enumerate()
        .map(|(at, (name, version))| format!("C:Q1{at:0>27}=\nP:{name}\nV:{version}\n\n"))
        .collect();
    index::read(Dialect::V3_0, &text)
        .map(|located| located.unwrap().into_record())
        .collect()
}

/// The dialect for a release shipping `apk-tools` at `version`, and whether
/// it was measured, or the kind of error.
fn chosen(version: &str) -> Result<(Dialect, bool), DialectErrorKind> {
    Dialect::for_release(&records(&[("apk-tools", version)]))
        .map(|selection| (selection.dialect(), selection.is_measured()))
        .map_err(|error| error.kind())
}

#[test]
fn the_measured_release_and_its_rebuilds_are_measured() {
    assert_eq!(chosen("3.0.9-r0"), Ok((Dialect::V3_0, true)));
    assert_eq!(chosen("3.0.9-r3"), Ok((Dialect::V3_0, true)));
    assert_eq!(chosen("3.0.9"), Ok((Dialect::V3_0, true)));
}

#[test]
fn another_release_of_the_series_is_answered_unmeasured() {
    assert_eq!(chosen("3.0.10-r0"), Ok((Dialect::V3_0, false)));
    assert_eq!(chosen("3.0.8-r1"), Ok((Dialect::V3_0, false)));
    assert_eq!(chosen("3.0.10_rc1-r0"), Ok((Dialect::V3_0, false)));
    assert_eq!(chosen("3.0.9.1-r0"), Ok((Dialect::V3_0, false)));
    assert_eq!(chosen("3.0-r0"), Ok((Dialect::V3_0, false)));
}

#[test]
fn a_series_no_dialect_follows_is_refused() {
    for version in [
        "2.14.13-r0",
        "3.1.0_rc1-r0",
        "3.1.0-r0",
        "4.0.0-r0",
        "30.0-r0",
    ] {
        assert_eq!(
            chosen(version),
            Err(DialectErrorKind::NoDialect),
            "{version}"
        );
    }
}

#[test]
fn a_version_outside_the_grammar_is_refused() {
    assert_eq!(chosen("3.0.9b1-r0"), Err(DialectErrorKind::NoDialect));
}

#[test]
fn the_error_names_the_version() {
    let error = Dialect::for_release(&records(&[("apk-tools", "3.1.0_rc1-r0")])).unwrap_err();
    assert_eq!(error.apk_tools().unwrap().as_bytes(), b"3.1.0_rc1-r0");
    assert_eq!(
        format!("{error}"),
        "no dialect follows apk-tools 3.1.0_rc1-r0"
    );
}

#[test]
fn only_the_package_named_apk_tools_counts() {
    let release = records(&[
        ("apk-tools-static", "3.0.9-r0"),
        ("apk-tools-doc", "3.0.9-r0"),
        ("musl", "1.2.5-r10"),
    ]);
    let error = Dialect::for_release(&release).unwrap_err();
    assert_eq!(error.kind(), DialectErrorKind::NoApkTools);
    assert_eq!(error.apk_tools(), None);
    assert_eq!(
        format!("{error}"),
        "no apk-tools package among the release's records"
    );
}

#[test]
fn the_highest_of_several_apk_tools_answers() {
    let release = records(&[
        ("apk-tools", "3.0.9-r0"),
        ("apk-tools", "3.1.0-r0"),
        ("apk-tools", "3.0.8-r0"),
    ]);
    let error = Dialect::for_release(&release).unwrap_err();
    assert_eq!(error.apk_tools().unwrap().as_bytes(), b"3.1.0-r0");
    let release = records(&[("apk-tools", "3.0.8-r0"), ("apk-tools", "3.0.9-r1")]);
    let selection = Dialect::for_release(&release).unwrap();
    assert_eq!(selection.apk_tools().as_bytes(), b"3.0.9-r1");
}

#[test]
fn upstream_drops_only_a_revision() {
    assert_eq!(upstream(b"3.0.9-r0"), b"3.0.9");
    assert_eq!(upstream(b"3.0.9-r12"), b"3.0.9");
    assert_eq!(upstream(b"3.0.9"), b"3.0.9");
    assert_eq!(upstream(b"3.0.9-r"), b"3.0.9-r");
    assert_eq!(upstream(b"3.0.9-rx"), b"3.0.9-rx");
    assert_eq!(upstream(b"-r1"), b"");
}
