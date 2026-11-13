use std::format;
use std::string::{String, ToString};
use std::vec;
use std::vec::Vec;

use super::*;
use crate::index::{Located, read};

// The rules themselves are held to apk's answers by the behavior catalog
// (`scenario/tests/catalog.rs`); these tests cover the interface.

/// The records of an index's text, each given an identity of its own where
/// it states none: one from its text and its place, so that records of two
/// calls are two packages unless they are one record.
fn records(text: &str) -> Vec<Record> {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut completed = String::new();
    for (at, record) in text.split("\n\n").enumerate() {
        if !record.contains("C:") {
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ at as u64;
            for &byte in record.as_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            let digits: String = (0..10)
                .map(|place| char::from(DIGITS[((hash >> (6 * place)) & 63) as usize]))
                .collect();
            completed.push_str(&format!("C:Q1{digits:A<27}=\n"));
        }
        completed.push_str(record.trim_end());
        completed.push_str("\n\n");
    }
    read(Dialect::V3_0, &completed)
        .map(|item| item.map(Located::into_record))
        .collect::<Result<_, _>>()
        .unwrap()
}

fn request(entries: &[&str]) -> Request {
    entries
        .iter()
        .map(|entry| Dialect::V3_0.dependency(entry).unwrap())
        .collect()
}

fn installed(resolution: &Resolution<'_>) -> Vec<String> {
    resolution
        .packages()
        .iter()
        .map(|record| format!("{}-{}", record.name(), record.version()))
        .collect()
}

#[test]
fn a_resolution_lists_its_packages_in_install_order() {
    let records = records("P:b\nV:1\nD:a\n\nP:a\nV:2\nD:c\n\nP:c\nV:3");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["b"])).unwrap();
    assert_eq!(installed(&resolution), ["c-3", "a-2", "b-1"]);
    assert_eq!(resolver.dialect(), Dialect::V3_0);
}

#[test]
fn an_empty_request_installs_nothing() {
    let records = records("P:a\nV:1");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    assert!(Request::new().is_empty());
    let resolution = resolver.resolve(&Request::new()).unwrap();
    assert!(resolution.packages().is_empty());
}

#[test]
fn the_later_of_two_entries_for_a_name_holds() {
    let records = records("P:a\nV:1.0\n\nP:a\nV:2.0");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["a", "a<2"])).unwrap();
    assert_eq!(installed(&resolution), ["a-1.0"]);
    let resolution = resolver.resolve(&request(&["a<2", "a"])).unwrap();
    assert_eq!(installed(&resolution), ["a-2.0"]);
}

#[test]
fn a_name_nothing_mentions_is_refused() {
    let records = records("P:a\nV:1");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["a", "nothing"])).unwrap_err();
    assert_eq!(refusal.kind(), RefusalKind::Unmet);
    assert_eq!(refusal.requirement().name(), "nothing");
    assert_eq!(refusal.requirement().by(), &RequiredBy::Request);
    assert_eq!(
        refusal.to_string(),
        "nothing, required by the request, which nothing can answer"
    );
}

#[test]
fn a_refusal_names_the_package_that_required_the_name() {
    let records = records("P:a\nV:1\nD:missing>=2");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["a"])).unwrap_err();
    assert_eq!(refusal.kind(), RefusalKind::Unmet);
    assert_eq!(refusal.requirement().name(), "missing");
    assert_eq!(
        refusal
            .requirement()
            .entry()
            .map(ToString::to_string)
            .as_deref(),
        Some("missing>=2")
    );
    assert!(
        matches!(refusal.requirement().by(), RequiredBy::Package(package) if package.name() == "a")
    );
    assert_eq!(
        refusal.to_string(),
        "missing>=2, required by a-1, which nothing can answer"
    );
}

#[test]
fn a_provider_without_priority_is_not_chosen() {
    let records = records("P:p\nV:1\np:x");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["x"])).unwrap_err();
    assert_eq!(refusal.kind(), RefusalKind::NoPriority);
    assert_eq!(refusal.requirement().name(), "x");
}

#[test]
fn a_package_a_decision_rules_out_is_unmet() {
    // Installing `a` requires `v=1`, which rules out `p2`, and with it `b`,
    // whose `v=2` only `p2` could meet.
    let records =
        records("P:a\nV:1\nD:v=1\n\nP:b\nV:1\nD:v=2\n\nP:p1\nV:1\np:v=1\n\nP:p2\nV:1\np:v=2");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["a", "b"])).unwrap_err();
    assert_eq!(refusal.kind(), RefusalKind::Unmet);
    assert_eq!(refusal.requirement().name(), "b");
}

#[test]
fn a_requirement_an_installed_provider_does_not_meet_is_a_conflict() {
    // `p1`, installed first (a carried name), provides `x`; `b1`, chosen
    // for `b` after it, needs `x` at 1.0, which binds `p1` too. `b` has two
    // candidates, so their dependency on `x` is shared without its
    // constraint, and nothing provides `x` at 1.0, so nothing rules `b1`
    // out beforehand.
    let records = records(
        "P:a\nV:1\nD:b p1\n\nP:p1\nV:1\np:x\nk:5\n\n\
         P:b1\nV:1\np:b\nk:10\nD:x=1.0\n\nP:b2\nV:1\np:b\nk:5\nD:x=1.0",
    );
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["a"])).unwrap_err();
    assert_eq!(refusal.kind(), RefusalKind::Conflict);
    assert_eq!(refusal.requirement().name(), "x");
    assert_eq!(refusal.holder().map(Package::name).unwrap(), "p1");
    assert_eq!(
        refusal.holder().map(ToString::to_string).as_deref(),
        Some("p1-1")
    );
    assert_eq!(
        refusal.to_string(),
        "x=1.0, required by b1-1, conflicts with p1-1, which holds the name"
    );
}

#[test]
fn a_package_whose_own_name_strands_it_is_never_a_candidate() {
    // `a` would hold `v` at 1, and needs it at 2: it can never be installed.
    let records = records("P:a\nV:1\np:v=1\nD:v=2\n\nP:p2\nV:1\np:v=2");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["a"])).unwrap_err();
    assert_eq!(refusal.kind(), RefusalKind::Unmet);
    assert_eq!(refusal.requirement().name(), "a");
}

#[test]
fn a_conflict_with_an_installed_package_is_forbidden() {
    let records = records("P:base\nV:1\n\nP:docs\nV:1\n\nP:extra\nV:1\ni:base\nD:!docs");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["base", "docs"])).unwrap_err();
    assert_eq!(refusal.kind(), RefusalKind::Forbidden);
    assert_eq!(refusal.requirement().name(), "docs");
    assert_eq!(refusal.holder().map(Package::name).unwrap(), "docs");
    assert_eq!(
        refusal.requirement().by(),
        &RequiredBy::Package(Package::of(&records[2]))
    );
    assert_eq!(
        refusal.to_string(),
        "!docs, stated by extra-1, forbids docs-1, which is installed"
    );
}

#[test]
fn a_requirement_says_which_records_answer_it() {
    // `zlib` provides `lib` at no version, which answers `lib<2` and not
    // `lib>=2`; `lib2` provides it at 2; `lib` 1 carries it at 1.
    let offered = records(
        "P:app\nV:1\nD:lib>=2\n\nP:lib\nV:1\n\nP:zlib\nV:1\np:lib\n\n\
         P:lib2\nV:1\np:lib=2\n\nP:old\nV:1\nD:lib<2",
    );
    let resolver = Resolver::new(Dialect::V3_0).repository(&offered[..2]);
    let refusal = resolver.resolve(&request(&["app"])).unwrap_err();
    let requirement = refusal.requirement();
    assert_eq!(requirement.to_string(), "lib>=2, required by app-1");
    let answering = |requirement: &Requirement| -> Vec<&Name> {
        offered
            .iter()
            .filter(|record| requirement.answered_by(record))
            .map(Record::name)
            .collect()
    };
    assert_eq!(answering(requirement), ["lib2"]);
    let resolver = Resolver::new(Dialect::V3_0).repository(&offered[2..]);
    let refusal = resolver.resolve(&request(&["old"])).unwrap_err();
    assert_eq!(answering(refusal.requirement()), ["lib", "zlib"]);
    // A `!` entry is answered by what it forbids.
    let forbidding = records("P:base\nV:1\n\nP:docs\nV:1\n\nP:extra\nV:1\ni:base\nD:!docs");
    let resolver = Resolver::new(Dialect::V3_0).repository(&forbidding);
    let refusal = resolver.resolve(&request(&["base", "docs"])).unwrap_err();
    assert!(refusal.requirement().answered_by(&forbidding[1]));
    assert!(!refusal.requirement().answered_by(&forbidding[0]));
}

#[test]
fn an_identity_requests_one_build() {
    let first = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\nV:1\nD:from-first";
    let second = "C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:a\nV:1\nD:from-second";
    let records = records(&format!(
        "{first}\n\n{second}\n\nP:from-first\nV:1\n\nP:from-second\nV:1"
    ));
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["a"])).unwrap();
    assert_eq!(installed(&resolution), ["from-first-1", "a-1"]);
    let resolution = resolver
        .resolve(&request(&["a><Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB="]))
        .unwrap();
    assert_eq!(installed(&resolution), ["from-second-1", "a-1"]);
}

#[test]
fn a_package_offered_again_is_one_package() {
    let records = records("C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\nV:1");
    let again = records.clone();
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&records)
        .repository(&again);
    let resolution = resolver.resolve(&request(&["a"])).unwrap();
    assert_eq!(installed(&resolution), ["a-1"]);
    assert!(core::ptr::eq(resolution.packages()[0], &records[0]));
}

#[test]
fn a_record_apk_cannot_install_is_never_chosen() {
    let records = records("P:a\nV:2\nD:x<\n\nP:a\nV:1");
    assert!(!records[0].is_installable());
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["a"])).unwrap();
    assert_eq!(installed(&resolution), ["a-1"]);
}

#[test]
fn a_dependency_cycle_resolves() {
    let records = records("P:a\nV:1\nD:b\n\nP:b\nV:1\nD:a");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["a"])).unwrap();
    assert_eq!(installed(&resolution), ["b-1", "a-1"]);
}

#[test]
fn order_places_a_set_after_what_each_depends_on() {
    let records = records("P:app\nV:1\nD:lib tool\n\nP:tool\nV:1\nD:lib\n\nP:lib\nV:1");
    assert_eq!(order(&records, &request(&["app"])), [2, 1, 0]);
}

#[test]
fn order_places_a_package_given_twice_with_itself() {
    let records = records("C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:lib\nV:1\n\nP:app\nV:1\nD:lib");
    let twice: Vec<&Record> = vec![&records[1], &records[0], &records[0]];
    assert_eq!(order(twice, &request(&["app"])), [1, 2, 0]);
}

#[test]
fn order_places_what_the_walk_does_not_reach_last() {
    let records = records("P:stray\nV:1\n\nP:app\nV:1\nD:lib\n\nP:lib\nV:1");
    assert_eq!(order(&records, &request(&["app"])), [2, 1, 0]);
}

fn changes(resolution: &Resolution<'_>) -> Vec<String> {
    resolution
        .changes()
        .iter()
        .map(|change| match *change {
            Change::Install { package, .. } => {
                format!("install {}-{}", package.name(), package.version())
            }
            Change::Upgrade { from, to, .. } => {
                format!("upgrade {} {} {}", to.name(), from.version(), to.version())
            }
            Change::Downgrade { from, to, .. } => {
                format!(
                    "downgrade {} {} {}",
                    to.name(),
                    from.version(),
                    to.version()
                )
            }
            Change::Replace { from, to, .. } => {
                format!("replace {} {} {}", to.name(), from.version(), to.version())
            }
            Change::Remove(record) => format!("remove {}-{}", record.name(), record.version()),
        })
        .collect()
}

#[test]
fn an_empty_root_s_changes_install_its_packages_in_order() {
    let records = records("P:b\nV:1\nD:a\n\nP:a\nV:2");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["b"])).unwrap();
    assert_eq!(changes(&resolution), ["install a-2", "install b-1"]);
}

#[test]
fn a_root_s_changes_upgrade_install_and_remove() {
    let offered = records("P:lib\nV:2.0\n\nP:foo\nV:1.0\nD:lib>=2");
    // Identities of their own: `records` numbers them from the first.
    let root = records(
        "C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:lib\nV:1.0\n\n\
         C:Q1JJJJJJJJJJJJJJJJJJJJJJJJJJJ=\nP:a\nV:1.0",
    );
    let world = [Dialect::V3_0.dependency("lib").unwrap()];
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&offered)
        .installed(&root, &world);
    let resolution = resolver.resolve(&request(&["foo"])).unwrap();
    assert_eq!(
        changes(&resolution),
        ["remove a-1.0", "upgrade lib 1.0 2.0", "install foo-1.0"]
    );
    assert_eq!(installed(&resolution), ["lib-2.0", "foo-1.0"]);
}

#[test]
fn an_installed_set_may_come_before_or_after_the_repositories() {
    let offered = records("P:v1\nV:1\np:v\nk:20\n\nP:b\nV:1");
    let root = records("C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:v2\nV:1\np:v\nk:20");
    let world = [Dialect::V3_0.dependency("v").unwrap()];
    let after = Resolver::new(Dialect::V3_0)
        .repository(&offered)
        .installed(&root, &world);
    let before = Resolver::new(Dialect::V3_0)
        .installed(&root, &world)
        .repository(&offered);
    let request = request(&["b"]);
    // The installed provider is listed first either way, and keeps the tie.
    assert_eq!(after.resolve(&request), before.resolve(&request));
    assert_eq!(changes(&after.resolve(&request).unwrap()), ["install b-1"]);
}

#[test]
fn a_world_entry_nothing_answers_is_refused_as_the_world_s() {
    let offered = records("P:b\nV:1");
    let world = [Dialect::V3_0.dependency("missing").unwrap()];
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&offered)
        .installed([], &world);
    let refusal = resolver.resolve(&request(&["b"])).unwrap_err();
    assert_eq!(refusal.requirement().name(), "missing");
    assert_eq!(refusal.requirement().by(), &RequiredBy::World);
    assert_eq!(
        refusal.to_string(),
        "missing, required by the world, which nothing can answer"
    );
}

#[test]
fn the_request_replaces_the_world_s_entry_for_a_name() {
    let offered = records("P:a\nV:1.0\n\nP:a\nV:2.0");
    let root = records("C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:a\nV:1.0");
    let world = [Dialect::V3_0.dependency("a<2").unwrap()];
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&offered)
        .installed(&root, &world);
    let resolution = resolver.resolve(&request(&["a>=2"])).unwrap();
    assert_eq!(changes(&resolution), ["upgrade a 1.0 2.0"]);
}

#[test]
fn order_walks_what_the_request_does_not_reach() {
    // `stray` is reached by nothing requested, and still follows `dep`.
    let records = records("P:stray\nV:1\nD:dep\n\nP:app\nV:1\n\nP:dep\nV:1");
    assert_eq!(order(&records, &request(&["app"])), [1, 2, 0]);
}

fn explained(resolution: &Resolution<'_>, record: &Record) -> Vec<String> {
    resolution
        .explain(record)
        .unwrap()
        .steps()
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_package_is_explained_back_to_the_request() {
    let records =
        records("P:app\nV:1\nD:lib>=2\n\nP:lib\nV:2\nD:so:libc.so\n\nP:musl\nV:1\np:so:libc.so=1");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["app"])).unwrap();
    assert_eq!(
        explained(&resolution, &records[2]),
        [
            "musl-1 is installed for so:libc.so, required by lib-2",
            "lib-2 is installed for lib>=2, required by app-1",
            "app-1 is installed for app, required by the request",
        ]
    );
    let Some(Reason::Required(requirement)) = resolution.reason(&records[2]) else {
        panic!("musl is installed for a requirement");
    };
    assert_eq!(requirement.name(), "so:libc.so");
    assert_eq!(
        requirement.entry().map(ToString::to_string).as_deref(),
        Some("so:libc.so")
    );
    assert_eq!(
        requirement.by(),
        &RequiredBy::Package(Package::of(&records[1]))
    );
    assert!(resolution.exclusion(&records[2]).is_none());
}

#[test]
fn a_record_outside_the_resolution_has_no_explanation() {
    let records = records("P:a\nV:1\n\nP:b\nV:1");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["a"])).unwrap();
    assert!(resolution.explain(&records[1]).is_none());
    assert!(resolution.reason(&records[1]).is_none());
}

#[test]
fn a_package_installed_for_its_condition_names_what_meets_it() {
    let records = records("P:app\nV:1\n\nP:docs\nV:1\n\nP:app-doc\nV:1\ni:app docs");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["app", "docs"])).unwrap();
    assert_eq!(
        resolution.reason(&records[2]),
        Some(&Reason::Condition(vec![
            Package::of(&records[0]),
            Package::of(&records[1])
        ]))
    );
    assert_eq!(
        explained(&resolution, &records[2]),
        [
            "app-doc-1 is installed for its condition, met by app-1, docs-1",
            "docs-1 is installed for docs, required by the request",
            "app-1 is installed for app, required by the request",
        ]
    );
}

#[test]
fn a_name_every_candidate_needs_is_explained_through_that_name() {
    // `lib`, which both candidates for `v` need, is required as soon as
    // `app` 2 requires `v`, and decided before `v`, which only packages
    // provide.
    let records = records(
        "P:app\nV:1\n\nP:app\nV:2\nD:v\n\nP:p1\nV:1\np:v\nk:10\nD:lib\n\n\
         P:p2\nV:1\np:v\nk:20\nD:lib\n\nP:lib\nV:1",
    );
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let resolution = resolver.resolve(&request(&["app"])).unwrap();
    assert_eq!(
        explained(&resolution, &records[4]),
        [
            "lib-1 is installed for lib, required by every candidate for v",
            "v, required by app-2",
            "app-2 is installed for app, required by the request",
        ]
    );
    let explanation = resolution.explain(&records[4]).unwrap();
    assert!(matches!(
        explanation.steps()[1],
        Step::Required(ref requirement) if requirement.name() == "v"
    ));
}

#[test]
fn a_refusal_explains_the_requirement_and_what_was_passed_over() {
    let records = records("P:app\nV:1\nD:lib>=2\n\nP:lib\nV:1");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["app"])).unwrap_err();
    assert_eq!(
        refusal.to_string(),
        "lib>=2, required by app-1, which nothing can answer"
    );
    let steps: Vec<String> = refusal
        .explanation()
        .steps()
        .iter()
        .map(ToString::to_string)
        .collect();
    // Each step comes before what it follows from. `app-1`, the only
    // candidate for `app`, requires `lib>=2` before it is installed.
    assert_eq!(
        steps,
        [
            "lib-1 is ruled out: lib>=2, required by app-1, does not admit it",
            "app-1 is the only candidate left for app, required by the request",
        ]
    );
    assert_eq!(refusal.explanation().to_string(), steps.join("\n"));
}

#[test]
fn a_refusal_the_request_states_explains_nothing_further() {
    let records = records("P:a\nV:1");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["nothing"])).unwrap_err();
    assert!(refusal.explanation().is_empty());
}

#[test]
fn a_conflict_explains_the_holder_too() {
    let records = records(
        "P:a\nV:1\nD:b p1\n\nP:p1\nV:1\np:x\nk:5\n\n\
         P:b1\nV:1\np:b\nk:10\nD:x=1.0\n\nP:b2\nV:1\np:b\nk:5\nD:x=1.0",
    );
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request(&["a"])).unwrap_err();
    let steps: Vec<String> = refusal
        .explanation()
        .steps()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        steps,
        [
            "b1-1 is installed for b, required by a-1",
            "p1-1 is installed for p1, required by a-1",
            "a-1 is installed for a, required by the request",
        ]
    );
}

#[test]
fn a_root_s_changes_are_explained() {
    let offered = records("P:lib\nV:2.0\n\nP:foo\nV:1.0\nD:lib>=2\n\nP:app\nV:1.0\ni:docs");
    let root = records(
        "C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:lib\nV:1.0\n\n\
         C:Q1JJJJJJJJJJJJJJJJJJJJJJJJJJJ=\nP:a\nV:1.0\n\n\
         C:Q1KKKKKKKKKKKKKKKKKKKKKKKKKKK=\nP:base\nV:1.0\n\n\
         C:Q1LLLLLLLLLLLLLLLLLLLLLLLLLLL=\nP:base-doc\nV:1.0\ni:base doc\n\n\
         C:Q1MMMMMMMMMMMMMMMMMMMMMMMMMMM=\nP:doc\nV:1.0\n\n\
         C:Q1NNNNNNNNNNNNNNNNNNNNNNNNNNN=\nP:old\nV:1.0\nX:1",
    );
    let world = [
        Dialect::V3_0.dependency("lib").unwrap(),
        Dialect::V3_0.dependency("base").unwrap(),
        Dialect::V3_0.dependency("doc").unwrap(),
        Dialect::V3_0.dependency("old").unwrap(),
    ];
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&offered)
        .installed(&root, &world);
    let resolution = resolver
        .resolve(&request(&["foo", "!doc", "!old"]))
        .unwrap();
    assert_eq!(
        changes(&resolution),
        [
            "remove a-1.0",
            "remove base-doc-1.0",
            "remove doc-1.0",
            "remove old-1.0",
            "upgrade lib 1.0 2.0",
            "install foo-1.0",
        ]
    );
    // Kept, as any package is installed: for the requirement that keeps it.
    assert_eq!(
        explained(&resolution, &root[2]),
        ["base-1.0 is kept for base, required by the world"]
    );
    // Upgraded: the package installed says why its name is needed, the
    // package replaced why it goes.
    assert_eq!(
        explained(&resolution, &offered[0]),
        ["lib-2.0 is installed for lib, required by the world"]
    );
    assert_eq!(
        explained(&resolution, &root[0]),
        [
            "lib-1.0 is dropped: lib>=2, required by foo-1.0, does not admit it",
            "foo-1.0 is the only candidate left for foo, required by the request",
        ]
    );
    // Removed: nothing requires it, a `!` entry forbids it, its condition
    // no longer holds.
    assert_eq!(resolution.exclusion(&root[1]), Some(&Exclusion::Unrequired));
    assert_eq!(
        explained(&resolution, &root[4]),
        ["doc-1.0 is dropped: !doc, stated by the request, forbids it"]
    );
    assert_eq!(
        explained(&resolution, &root[3]),
        [
            "base-doc-1.0 is dropped: its condition does not hold",
            "doc-1.0 is dropped: !doc, stated by the request, forbids it",
        ]
    );
    assert_eq!(
        resolution.exclusion(&root[5]),
        Some(&Exclusion::Uninstallable)
    );
    assert!(resolution.reason(&root[0]).is_none());
}

#[test]
fn a_world_s_conflict_is_the_world_s() {
    let offered = records("P:app\nV:1\nD:lib\n\nP:lib\nV:1");
    let world = [Dialect::V3_0.dependency("!lib").unwrap()];
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&offered)
        .installed([], &world);
    let refusal = resolver.resolve(&request(&["app"])).unwrap_err();
    let steps: Vec<String> = refusal
        .explanation()
        .steps()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        steps,
        [
            "app-1 is ruled out: its dependency lib admits only packages ruled out",
            "lib-1 is ruled out: !lib, stated by the world, forbids it",
        ]
    );
}

#[test]
fn a_record_with_the_identity_of_one_the_root_holds_is_that_one() {
    // The root's record of `a` is one apk cannot install, and a repository
    // offers one of its identity that it could: that is the root's package,
    // which apk cannot install, so the world's `a` is refused.
    let offered = records("C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:a\nV:1");
    let root = records("C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:a\nV:1\nX:1");
    assert!(!root[0].is_installable());
    let world = [Dialect::V3_0.dependency("a").unwrap()];
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&offered)
        .installed(&root, &world);
    let refusal = resolver.resolve(&request(&["a"])).unwrap_err();
    assert_eq!(refusal.requirement().name(), "a");
}

#[test]
fn a_package_the_root_drops_is_named_as_its_change_names_it() {
    // The root's record of `a` and a repository's share an identity, so
    // they are one package, and the root's is the one removed.
    let offered = records("C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:a\nV:2\n\nP:b\nV:1");
    let root = records("C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:a\nV:1");
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&offered)
        .installed(&root, []);
    let resolution = resolver.resolve(&request(&["b"])).unwrap();
    assert_eq!(changes(&resolution), ["remove a-1", "install b-1"]);
    assert_eq!(
        explained(&resolution, &root[0]),
        ["a-1 is dropped: nothing requires it"]
    );
    assert_eq!(resolution.source(&offered[0]), Some(Source::Installed));
}

// apk.static 3.0.9 resolves names that are not UTF-8 as it resolves any
// other: each answer below is its own, to the same records.

/// The records of an index's bytes, each `P:` line given an identity of its
/// own.
fn records_of(text: &[u8]) -> Vec<Record> {
    let mut completed = Vec::new();
    for (at, line) in text.split_inclusive(|&byte| byte == b'\n').enumerate() {
        if line.starts_with(b"P:") {
            completed.extend_from_slice(format!("C:Q1{at:A<27}=\n").as_bytes());
        }
        completed.extend_from_slice(line);
    }
    read(Dialect::V3_0, &completed)
        .map(|item| item.map(Located::into_record))
        .collect::<Result<_, _>>()
        .unwrap()
}

fn request_of(entries: &[&[u8]]) -> Request {
    entries
        .iter()
        .map(|entry| Dialect::V3_0.dependency(entry).unwrap())
        .collect()
}

/// The packages of `request`'s resolution over `records`, in install order,
/// as `name-version`.
fn installs(records: &[Record], request: &Request) -> Vec<String> {
    let resolver = Resolver::new(Dialect::V3_0).repository(records);
    installed(&resolver.resolve(request).unwrap())
}

#[test]
fn names_that_are_not_utf8_are_names_byte_for_byte() {
    let records = records_of(b"P:a\xff\nV:1.0-r0\n\nP:a\xfe\nV:2.0-r0\n\nP:b\nV:1.0-r0\nD:a\xfe\n");
    assert_eq!(
        installs(&records, &request_of(&[b"b"])),
        [r"a\xfe-2.0-r0", "b-1.0-r0"]
    );
    assert_eq!(
        installs(&records, &request_of(&[b"a\xff"])),
        [r"a\xff-1.0-r0"]
    );
    let records = records_of(b"P:a\xff\nV:1.0-r0\n\nP:t\nV:1.0-r0\ni:a\xff\n");
    assert_eq!(
        installs(&records, &request_of(&[b"a\xff"])),
        [r"a\xff-1.0-r0", "t-1.0-r0"]
    );
    let records = records_of(b"P:p\nV:1.0-r0\np:x\xff=1.0\n\nP:b\nV:1.0-r0\nD:x\xff\n");
    assert_eq!(
        installs(&records, &request_of(&[b"b"])),
        ["p-1.0-r0", "b-1.0-r0"]
    );
}

#[test]
fn a_refusal_writes_each_name_apart_from_every_other() {
    // apk refuses `app`: nothing provides `lib\xff`, only `lib\xfe` and
    // `lib\xff` spelled out in four bytes.
    let records =
        records_of(b"P:app\nV:1.0-r0\nD:lib\xff\n\nP:x\nV:1\np:lib\xfe\n\nP:y\nV:1\np:lib\\xff\n");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request_of(&[b"app"])).unwrap_err();
    assert_eq!(
        refusal.to_string(),
        r"lib\xff, required by app-1.0-r0, which nothing can answer"
    );
    assert_eq!(
        refusal.explanation().to_string(),
        "app-1.0-r0 is the only candidate left for app, required by the request"
    );
    let records = records_of(b"P:app\nV:1.0-r0\nD:lib\\xff>=2\n\nP:lib\\xff\nV:1\n");
    let resolver = Resolver::new(Dialect::V3_0).repository(&records);
    let refusal = resolver.resolve(&request_of(&[b"app"])).unwrap_err();
    assert_eq!(
        refusal.to_string(),
        r"lib\\xff>=2, required by app-1.0-r0, which nothing can answer"
    );
    assert_eq!(
        refusal.explanation().to_string(),
        "lib\\\\xff-1 is ruled out: lib\\\\xff>=2, required by app-1.0-r0, does not admit it\n\
         app-1.0-r0 is the only candidate left for app, required by the request"
    );
}

#[test]
fn a_byte_that_is_not_ascii_orders_after_every_one_that_is() {
    // `x` is decided before `\xff`, and takes `p1`; decided first, `\xff`
    // would take `p2`, which provides `x` too.
    let records = records_of(
        b"P:p1\nV:1\np:x\nk:100\n\nP:p2\nV:1\np:x \xff=1\nk:10\n\nP:p3\nV:1\np:\xff=1\n",
    );
    assert_eq!(
        installs(&records, &request_of(&[b"\xff", b"x"])),
        ["p1-1", "p2-1"]
    );
    // Two requested packages install in the order of their names.
    let records = records_of(b"P:\xfflib\nV:1\n\nP:a\nV:1\n");
    assert_eq!(
        installs(&records, &request_of(&[b"\xfflib", b"a"])),
        ["a-1", r"\xfflib-1"]
    );
}

#[test]
fn a_package_comes_from_the_first_repository_offering_it() {
    let main = records("P:app\nV:1\nD:lib extra");
    // The same package again, its name and identity, and another.
    let mut extra = records("P:extra\nV:1");
    extra.insert(0, main[0].clone());
    let lib = records("C:Q1LLLLLLLLLLLLLLLLLLLLLLLLLLL=\nP:lib\nV:1");
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&main)
        .repository(&extra)
        .repository(&[])
        .repository(&lib);
    let resolution = resolver.resolve(&request(&["app"])).unwrap();
    assert_eq!(resolution.source(&main[0]), Some(Source::Repository(0)));
    assert_eq!(resolution.source(&extra[0]), Some(Source::Repository(0)));
    assert_eq!(resolution.source(&extra[1]), Some(Source::Repository(1)));
    assert_eq!(resolution.source(&lib[0]), Some(Source::Repository(3)));
    let elsewhere = records("C:Q1EEEEEEEEEEEEEEEEEEEEEEEEEEE=\nP:lib\nV:1");
    assert_eq!(resolution.source(&elsewhere[0]), None);
}

#[test]
fn a_package_the_root_held_comes_from_the_root() {
    let offered = records("P:lib\nV:2.0\n\nP:foo\nV:1.0\nD:lib>=2 keep");
    let root = records(
        "C:Q1IIIIIIIIIIIIIIIIIIIIIIIIIII=\nP:lib\nV:1.0\n\n\
         C:Q1JJJJJJJJJJJJJJJJJJJJJJJJJJJ=\nP:a\nV:1.0\n\n\
         C:Q1KKKKKKKKKKKKKKKKKKKKKKKKKKK=\nP:keep\nV:1.0",
    );
    // The root's `keep` is offered too: kept, it is the root's.
    let again = [root[2].clone()];
    let world = [Dialect::V3_0.dependency("lib").unwrap()];
    let resolver = Resolver::new(Dialect::V3_0)
        .repository(&again)
        .repository(&offered)
        .installed(&root, &world);
    let resolution = resolver.resolve(&request(&["foo"])).unwrap();
    assert_eq!(
        changes(&resolution),
        ["remove a-1.0", "upgrade lib 1.0 2.0", "install foo-1.0"]
    );
    assert_eq!(resolution.source(&offered[0]), Some(Source::Repository(1)));
    assert_eq!(resolution.source(&offered[1]), Some(Source::Repository(1)));
    assert_eq!(resolution.source(&root[0]), Some(Source::Installed));
    assert_eq!(resolution.source(&root[1]), Some(Source::Installed));
    assert_eq!(resolution.source(&root[2]), Some(Source::Installed));
    assert_eq!(resolution.source(&again[0]), Some(Source::Installed));
}
