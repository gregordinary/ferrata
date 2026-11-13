use std::string::ToString;

use super::*;

fn version(text: &str) -> Version {
    Dialect::V3_0
        .version(text)
        .unwrap_or_else(|error| panic!("{text:?}: {error}"))
}

/// Asserts that `a` comes before `b`, both ways round.
fn lt(a: &str, b: &str) {
    let (a_version, b_version) = (version(a), version(b));
    assert_eq!(a_version.cmp(&b_version), Ordering::Less, "{a} < {b}");
    assert_eq!(b_version.cmp(&a_version), Ordering::Greater, "{b} > {a}");
}

fn eq(a: &str, b: &str) {
    let (a_version, b_version) = (version(a), version(b));
    assert_eq!(a_version.cmp(&b_version), Ordering::Equal, "{a} = {b}");
    assert!(a_version == b_version, "{a} == {b}");
}

fn refused(text: &str) -> VersionError {
    match Dialect::V3_0.version(text) {
        Ok(_) => panic!("{text:?} was read as a version"),
        Err(error) => error,
    }
}

// Every ordering and every verdict on validity below is apk.static 3.0.9's,
// from `apk version -t` and `apk version -c`.

#[test]
fn parts_of_different_kinds_rank_by_their_place_in_the_grammar() {
    // At the position where the two first differ in kind: a pre-release
    // suffix, then running out, a revision, a commit, a post-release suffix,
    // a letter, and a further dotted number.
    let ladder = [
        "1.0_rc1", "1.0", "1.0-r1", "1.0~ff", "1.0_p1", "1.0a", "1.0.1",
    ];
    for pair in ladder.windows(2) {
        lt(pair[0], pair[1]);
    }
}

#[test]
fn suffixes_rank_by_their_table() {
    let ladder = [
        "1.0_alpha",
        "1.0_beta",
        "1.0_pre",
        "1.0_rc",
        "1.0",
        "1.0_cvs",
        "1.0_svn",
        "1.0_git",
        "1.0_hg",
        "1.0_p",
    ];
    for pair in ladder.windows(2) {
        lt(pair[0], pair[1]);
    }
}

#[test]
fn a_suffix_number_is_a_part_of_its_own() {
    lt("1.0_p", "1.0_p0");
    lt("1.0_p", "1.0_p1");
    lt("1.0_alpha", "1.0_alpha0");
    // A further post-release suffix comes earlier in the grammar than a
    // number, and a pre-release one lowers its version whatever it meets.
    lt("1.0_p1", "1.0_p_p");
    lt("1.0_rc0", "1.0_rc_p");
    lt("1.0_p_alpha", "1.0_p0");
    lt("1.0_p_alpha", "1.0_p");
    lt("1.0_p-r9", "1.0_p0");
    lt("1.0_p~ff", "1.0_p0");
    lt("1.0_p1-r1", "1.0_p1~ff");
    lt("1.0_alpha1", "1.0_alpha2");
    lt("1.0_alpha2", "1.0_alpha10");
}

#[test]
fn a_dotted_number_with_a_leading_zero_compares_as_text() {
    lt("1.01", "1.1");
    lt("1.05", "1.5");
    lt("1.0", "1.00");
    lt("1.00", "1.000");
    lt("1.018446744073709551615", "1.02");
    // The first number is compared by value whatever its spelling.
    eq("01.0", "1.0");
    eq("00", "0");
    eq("010", "10");
}

#[test]
fn other_numbers_compare_by_value() {
    lt("1.9", "1.10");
    lt("1.0.1", "1.1");
    lt("1.0z", "1.1");
    lt("1.0-r9", "1.0-r10");
    eq("1.0-r01", "1.0-r1");
    eq("1.0_p01", "1.0_p1");
    lt("18446744073709551614", "18446744073709551615");
    lt("1.0-r2", "1.0-r18446744073709551615");
}

#[test]
fn commits_compare_as_text() {
    lt("1.0~abc", "1.0~abd");
    lt("1.0~ab", "1.0~abc");
    lt("1.0~10", "1.0~9");
    lt("1.0~0", "1.0~00");
    lt("1.0~0f", "1.0~f");
    lt("1.0-r1", "1.0~ff");
    lt("1.0~ff", "1.0_p1");
}

#[test]
fn letters_compare_alphabetically() {
    lt("1.0a", "1.0b");
    lt("1.0a", "1.0z");
    lt("1.0a_p1", "1.0b");
}

#[test]
fn equal_versions_spelled_apart_are_equal() {
    let (a, b) = (version("1.0-r01"), version("1.0-r1"));
    assert_eq!(a, b);
    assert_eq!(a.as_bytes(), b"1.0-r01");
    assert_eq!(b.to_string(), "1.0-r1");
    assert_ne!(version("1.0"), version("1.0.0"));
}

#[test]
fn every_shape_the_grammar_defines_is_read() {
    for text in [
        "0",
        "00",
        "1",
        "1.0",
        "1a",
        "1.0z",
        "1_p1",
        "1~ff",
        "1-r0",
        "1.0_p",
        "1.0_pre",
        "1.0_alpha1_beta2",
        "1.0_p1_p2",
        "1.0_rc01",
        "1.0_p1~abc",
        "1.0~abc-r1",
        "1.0a_p1",
        "1.0a~ff-r3",
        "1.0a-r0",
        "1.0-r00",
        "18446744073709551615",
        "1.018446744073709551615",
        "000000000000000000000001",
        "1.000000000000000000000000001",
        "1.0-r000000000000000000000001",
    ] {
        version(text);
    }
}

#[test]
fn a_part_out_of_place_is_refused_where_it_stands() {
    for (text, offset) in [
        ("1.", 2),
        (".1", 0),
        ("1..0", 2),
        ("1.0.", 4),
        ("1A", 1),
        ("1.0a1", 4),
        ("1.0ab", 4),
        ("1a.1", 2),
        ("1.0_p1a", 6),
        ("1.0_p.1", 5),
        ("1.0~", 4),
        ("1.0~ABC", 4),
        ("1.0~abg", 6),
        ("1.0~abc_p1", 7),
        ("1.0~a~b", 5),
        ("1.0-r1~abc", 6),
        ("1.0-r1_p1", 6),
        ("1.0-r", 5),
        ("1.0-r1a", 6),
        ("1.0-r1-r2", 6),
        ("1.0-R1", 3),
        ("1.0-1", 3),
        ("1.0 ", 3),
        (" 1.0", 0),
        ("1.0+1", 3),
        ("1:1.0", 1),
        ("-r1", 0),
        ("a", 0),
        ("1.0ü", 3),
    ] {
        let error = refused(text);
        assert_eq!(error.kind(), VersionErrorKind::Unexpected, "{text:?}");
        assert_eq!(error.offset(), offset, "{text:?}");
    }
}

#[test]
fn a_suffix_the_table_does_not_hold_is_refused() {
    for (text, offset) in [
        ("1.0_pr", 4),
        ("1.0_alph", 4),
        ("1.0_P1", 4),
        ("1.0_", 4),
        ("1.0__p", 4),
        ("1.0_p_", 6),
    ] {
        let error = refused(text);
        assert_eq!(error.kind(), VersionErrorKind::UnknownSuffix, "{text:?}");
        assert_eq!(error.offset(), offset, "{text:?}");
    }
}

#[test]
fn a_number_past_sixty_four_bits_is_refused() {
    for (text, offset) in [
        ("18446744073709551616", 0),
        ("99999999999999999999", 0),
        ("1.18446744073709551616", 2),
        ("1.018446744073709551616", 2),
        ("1.099999999999999999999", 2),
        ("1.0-r18446744073709551616", 5),
        ("1.0_p18446744073709551616", 5),
    ] {
        let error = refused(text);
        assert_eq!(error.kind(), VersionErrorKind::TooLarge, "{text:?}");
        assert_eq!(error.offset(), offset, "{text:?}");
    }
}

#[test]
fn empty_text_is_refused() {
    assert_eq!(refused("").kind(), VersionErrorKind::Empty);
    assert_eq!(refused("").offset(), 0);
}

#[test]
fn an_error_says_what_and_where() {
    assert_eq!(
        refused("1.0-r1_p1").to_string(),
        "not a version: a character out of place at byte 6",
    );
}

#[test]
fn a_prefix_comparison_stops_where_the_second_version_does() {
    let prefix = |a: &str, b: &str| version(a).compare_prefix(&version(b));
    assert_eq!(prefix("1.0.5", "1.0"), Ordering::Equal);
    assert_eq!(prefix("1.0_rc1", "1.0"), Ordering::Equal);
    assert_eq!(prefix("1.0", "1.0"), Ordering::Equal);
    assert_eq!(prefix("1.1", "1.0"), Ordering::Greater);
    assert_eq!(prefix("1.0", "1.0.1"), Ordering::Less);
    // Parts, not characters: `1.10` does not begin with `1.1`.
    assert_eq!(prefix("1.10", "1.1"), Ordering::Greater);
}

/// A version as a package record holds it, which may be one the grammar does
/// not allow.
fn read(text: &str) -> Version {
    Version::read(Dialect::V3_0, text.as_bytes())
}

// Each ordering below is apk.static 3.0.9's, measured by offering two records
// of one package at the two versions, in both orders, and seeing which it
// installs. Where it installs the first listed either way, it orders the two
// as equal.

#[test]
fn a_version_outside_the_grammar_is_kept_and_marked() {
    for text in [
        "1.0b1-r0",
        "0..0",
        "",
        "abc",
        "1.0_foo",
        "1.18446744073709551616",
    ] {
        let version = read(text);
        assert_eq!(version.as_bytes(), text.as_bytes());
        assert!(!version.is_valid(), "{text:?}");
    }
    assert!(read("1.0-r0").is_valid());
    assert!(version("1.0-r0").is_valid());
}

#[test]
fn the_module_documentation_s_ladder_holds() {
    let ladder = [
        "1.0_rc1", "1.0_foo", "1.0", "1.0-r0", "1.0b1-r0", "1.0b1-r9", "1.0b", "1.0c",
    ];
    for pair in ladder.windows(2) {
        let expected = if pair == ["1.0b1-r0", "1.0b1-r9"] {
            Ordering::Equal
        } else {
            Ordering::Less
        };
        assert_eq!(read(pair[0]).cmp(&read(pair[1])), expected, "{pair:?}");
    }
}

#[test]
fn a_part_apk_cannot_read_ranks_after_running_out() {
    for (a, b) in [
        ("1.0..1", "1.0"),
        ("1.0..1", "1.0.1"),
        ("1.0_foo", "1.0"),
        ("1.0_foo", "1.0.1"),
        ("1.0_foo", "1.0_p1"),
        ("1.0b1-r0", "1.0b-r0"),
        ("1.2a1", "1.2a"),
        ("1.2a1", "1.2a-r0"),
        ("1.0-r", "1.0"),
        ("1.0~", "1.0"),
        ("1.0~xyz", "1.0~0"),
        ("1.0-r1_p1", "1.0-r1"),
        ("1.0a.1", "1.0a"),
        ("1.0_pr", "1.0_p"),
        ("1.0_alphaX", "1.0_alpha"),
        ("1.0_alpha1X", "1.0_alpha1"),
        ("1.18446744073709551616", "1.1"),
        ("1.18446744073709551616", "1"),
        ("1.0.18446744073709551616", "1.0"),
        ("1.0_p18446744073709551616", "1.0_p"),
        ("abc", "0"),
    ] {
        assert_eq!(read(a).cmp(&read(b)), Ordering::Less, "{a} < {b}");
        assert_eq!(read(b).cmp(&read(a)), Ordering::Greater, "{b} > {a}");
    }
}

#[test]
fn a_pre_release_suffix_still_ranks_first() {
    for (a, b) in [
        ("1.0_rc1", "1.0_foo"),
        ("1.0_alpha", "1.0..1"),
        ("1.0_rc1", "1.0-r"),
        ("1.0_rc1", "1.0-R1"),
        ("1.0_alpha", "1.0_P"),
        ("1.0_alpha", "1.0A"),
    ] {
        assert_eq!(read(a).cmp(&read(b)), Ordering::Less, "{a} < {b}");
        assert_eq!(read(b).cmp(&read(a)), Ordering::Greater, "{b} > {a}");
    }
}

#[test]
fn what_is_read_before_the_unreadable_part_still_decides() {
    for (a, b) in [
        ("1.0-r0", "1.0b1-r0"),
        ("0.9-r0", "1.0b1-r0"),
        ("1.0b1-r0", "1.0c-r0"),
        ("1.0a", "1.0b1"),
        ("1.0_alpha", "1.0b1"),
        ("1.0~aa", "1.0~abg"),
        ("1.0~abg", "1.0~ac"),
        ("1.0_p0", "1.0_p1a"),
        ("1.0_p1a", "1.0_p2"),
        ("1.0-r0", "1.0-r1a"),
        ("1.0-r1a", "1.0-r2"),
        ("1.0_alpha0", "1.0_alpha1X"),
        ("1.0_p", "1.0a.1"),
        ("1.0-r1_p1", "1.0-r2"),
        ("1.0_alpha", "1.0_p18446744073709551616"),
    ] {
        assert_eq!(read(a).cmp(&read(b)), Ordering::Less, "{a} < {b}");
    }
}

#[test]
fn versions_that_go_wrong_at_one_part_are_equal() {
    for (a, b) in [
        ("1.0b1-r0", "1.0b2-r0"),
        ("1.0b2-r0", "1.0b10-r0"),
        ("1.0b1-r1", "1.0b1-r0"),
        ("1.0b1", "1.0b1-r5"),
        ("1.18446744073709551616", "1.18446744073709551617"),
        ("abc", "abd"),
        ("", "abc"),
    ] {
        assert_eq!(read(a), read(b), "{a} = {b}");
    }
}

#[test]
fn a_prefix_comparison_reads_up_to_the_unreadable_part() {
    // apk.static 3.0.9 installs `foo` at 1.0b1-r0 for `foo~1.0`, `foo>=1.0`
    // and `foo<99`, and not for `foo<1.0`.
    let candidate = read("1.0b1-r0");
    assert_eq!(candidate.compare_prefix(&version("1.0")), Ordering::Equal);
    assert_eq!(candidate.cmp(&version("1.0")), Ordering::Greater);
    assert_eq!(candidate.cmp(&version("99")), Ordering::Less);
}
