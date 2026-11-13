//! Small repositories made at random, with a request over them.
//!
//! Each is a [`Scenario`] without an answer: a few packages whose names come
//! from a pool small enough that they meet, depend on each other, provide
//! each other's names and contend for them. Records carry the fields a
//! resolution turns on (versions, dependencies with constraints and `!`
//! conflicts, provides at a version and at none, priorities, and
//! `install_if` conditions), sometimes over two repositories, a package
//! offered in both or rebuilt in the second at the same version.
//! Dependencies, conditions and the request mostly name what some package
//! carries or provides, so that most requests are answered rather than
//! refused.
//!
//! What is made depends only on the [`Source`] of choices, so a case made
//! from a seed is made again from it, and a fuzzer's input steers what is
//! made.

use crate::format::{Record, Repository, Scenario, derived_identity};
use crate::random::Source;
use crate::resolve::entry_name;

/// Names packages carry, and which other packages may provide.
const PACKAGES: [&str; 6] = ["a", "b", "c", "d", "e", "f"];

/// Names only provided, never carried.
const PROVIDED: [&str; 3] = ["v", "w", "so:x"];

/// The versions packages are offered at, and names provided at.
const VERSIONS: [&str; 3] = ["1", "2", "3"];

/// What a dependency or a condition may state of the version.
const CONSTRAINTS: [&str; 6] = [">=2", "<2", "=1", "~2", ">1", "<3"];

/// The priorities a provider may state; the same one twice makes a tie.
const PRIORITIES: [&str; 4] = ["1", "10", "10", "100"];

/// The names the repositories mention: those packages carry, and those
/// they provide.
#[derive(Debug, Default)]
struct Names {
    carried: Vec<String>,
    provided: Vec<String>,
}

impl Names {
    /// A name some package carries or provides, or one time in `unknown`
    /// one none does.
    fn any(&self, source: &mut impl Source, unknown: usize) -> String {
        if source.one_in(unknown) || (self.carried.is_empty() && self.provided.is_empty()) {
            let pool: &[&str] = if source.one_in(2) {
                &PACKAGES
            } else {
                &PROVIDED
            };
            return source.pick(pool).to_owned();
        }
        if self.provided.is_empty() || (!self.carried.is_empty() && source.one_in(2)) {
            self.carried[source.below(self.carried.len())].clone()
        } else {
            self.provided[source.below(self.provided.len())].clone()
        }
    }

    /// A name some package carries, if any does.
    fn carried(&self, source: &mut impl Source) -> Option<String> {
        (!self.carried.is_empty()).then(|| self.carried[source.below(self.carried.len())].clone())
    }
}

/// `count` entries made by `make`, without two for one name.
fn distinct(count: usize, mut make: impl FnMut() -> Option<String>) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for _ in 0..count {
        let Some(entry) = make() else { continue };
        let name = entry_name(&entry);
        if !entries.iter().any(|kept| entry_name(kept) == name) {
            entries.push(entry);
        }
    }
    entries
}

/// `name`, now and then with a constraint, and as a conflict one time in
/// `conflicts`, if at all.
fn entry(source: &mut impl Source, name: String, conflicts: Option<usize>) -> String {
    let mut text = String::new();
    if conflicts.is_some_and(|odds| source.one_in(odds)) {
        text.push('!');
    }
    text.push_str(&name);
    if source.one_in(8) {
        text.push_str(source.pick(&CONSTRAINTS));
    }
    text
}

/// A package's name, version, priority and provides: what it is, before
/// what it needs.
fn package(source: &mut impl Source) -> Record {
    let name = source.pick(&PACKAGES);
    let mut fields = vec![
        ('P', name.to_owned()),
        ('V', source.pick(&VERSIONS).to_owned()),
    ];
    if source.one_in(2) {
        fields.push(('k', source.pick(&PRIORITIES).to_owned()));
    }
    let count = if source.one_in(2) {
        1 + source.below(2)
    } else {
        0
    };
    let provides = distinct(count, || {
        let provided = if source.one_in(4) {
            source.pick(&PACKAGES)
        } else {
            source.pick(&PROVIDED)
        };
        // A package providing its own name conflicts with itself, which is
        // worth meeting now and then, not in every other case.
        if provided == name && !source.one_in(10) {
            return None;
        }
        Some(if source.one_in(2) {
            format!("{provided}={}", source.pick(&VERSIONS))
        } else {
            provided.to_owned()
        })
    });
    if !provides.is_empty() {
        fields.push(('p', provides.join(" ")));
    }
    Record { fields }
}

/// Adds to `record` what it depends on and when it is installed by itself.
fn needs(source: &mut impl Source, record: &mut Record, names: &Names) {
    let count = source.below(4);
    let depends = distinct(count, || {
        let name = names.any(source, 10);
        Some(entry(source, name, Some(16)))
    });
    if !depends.is_empty() {
        insert(record, 'D', depends.join(" "));
    }
    if source.one_in(5) {
        let count = 1 + source.below(2);
        let condition = distinct(count, || {
            let name = names.carried(source)?;
            let mut text = String::new();
            if source.one_in(6) {
                text.push('!');
            }
            text.push_str(&name);
            if source.one_in(6) {
                text.push_str(source.pick(&CONSTRAINTS));
            }
            Some(text)
        });
        if !condition.is_empty() {
            record.fields.push(('i', condition.join(" ")));
        }
    }
}

/// Puts `letter` among `record`'s fields before its provides, where an
/// index puts it.
fn insert(record: &mut Record, letter: char, value: String) {
    let at = record
        .fields
        .iter()
        .position(|(field, _)| *field == 'p')
        .unwrap_or(record.fields.len());
    record.fields.insert(at, (letter, value));
}

/// A scenario: one or two repositories of a few packages each, and a
/// request of one to three entries, with no answer and no statement.
pub fn scenario(source: &mut impl Source) -> Scenario {
    let mut first: Vec<Record> = (0..2 + source.below(6)).map(|_| package(source)).collect();
    // Two records of one name and version in one repository are one package
    // to apk unless their identities differ; keep the first of each.
    dedup(&mut first);
    let mut second: Vec<Record> = Vec::new();
    if source.one_in(4) {
        second = (0..1 + source.below(3)).map(|_| package(source)).collect();
    }

    let mut names = Names::default();
    for record in first.iter().chain(&second) {
        names.carried.push(record.name().to_owned());
        for (letter, value) in &record.fields {
            if *letter == 'p' {
                let provided = value.split(' ').filter_map(entry_name);
                names.provided.extend(provided);
            }
        }
    }
    for record in first.iter_mut().chain(&mut second) {
        needs(source, record, &names);
    }

    let mut repositories = vec![Repository::Records(first.clone())];
    if !second.is_empty() {
        // The same package offered again, or a second build of its version.
        if source.one_in(2) {
            let mut again = first[source.below(first.len())].clone();
            if source.one_in(2) {
                let rebuilt = format!("{} rebuilt", again.version());
                let identity = derived_identity(again.name(), &rebuilt);
                again.fields.insert(0, ('C', identity));
            }
            second.push(again);
        }
        dedup(&mut second);
        repositories.push(Repository::Records(second));
    }

    let count = 1 + source.below(3);
    let mut request = distinct(count, || {
        let name = names.any(source, 40);
        Some(entry(source, name, None))
    });
    if source.one_in(10) {
        let conflict = format!("!{}", names.any(source, 10));
        if !request
            .iter()
            .any(|kept| entry_name(kept) == entry_name(&conflict))
        {
            request.push(conflict);
        }
    }
    Scenario {
        repositories,
        request,
        ..Scenario::default()
    }
}

/// Keeps the first record of each name and version.
fn dedup(records: &mut Vec<Record>) {
    let mut seen: Vec<(String, String)> = Vec::new();
    records.retain(|record| {
        let key = (record.name().to_owned(), record.version().to_owned());
        let fresh = !seen.contains(&key);
        seen.push(key);
        fresh
    });
}
