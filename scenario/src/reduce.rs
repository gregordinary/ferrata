//! Shrinking a scenario on which apk and a second resolver disagree.
//!
//! The reducer removes what it can from a scenario, a part at a time, and
//! keeps each removal while apk and the second resolver still disagree in
//! the way they did at the start: the same side refusing, the packages
//! named with `--keep` still on the side they were on, both sides making
//! the same steps in another order where they did at the start, and other
//! steps where they did not, so that a disagreement about order is not
//! reduced to one about what is changed; and, comparing sequences, the
//! sequences first parting at the same steps, so that a reduction of one
//! order difference does not drift to another the root holds. It removes, in turn, records; field lines other than `C:`, `P:`,
//! `V:` and `A:`, and entries of `D:`, `p:` and `i:`, of repository and
//! installed records alike; installed records; world entries; and request
//! entries, but never the last of them, since `apk add` with nothing to add
//! is not the request the scenario made; and repeats until a whole round
//! removes nothing. Asked to keep the root consistent, it removes nothing
//! that leaves the installed set as apk would not have made it: an installed
//! package with a dependency no installed package answers, one another
//! installed package's `!` entry forbids, or a world entry no installed
//! package answers, or for a `!` entry, one an installed package answers.
//! Each kind is cut down in chunks: halves first, then quarters, down to
//! single parts.
//!
//! The second resolver is a command. The reducer appends the path of a
//! directory holding the scenario, as [`materialize`] lays it out, and reads
//! the command's standard output as an answer: one step to a line, as a
//! scenario's answer section writes them, or `refused`. A command that exits
//! with failure has not answered, and the scenario it failed on is treated
//! as one that does not show the disagreement.

use std::collections::BTreeSet;
use std::io;
use std::process::Command;

use ferrata::dependency::Dependency;

use crate::apk::{Apk, TempDir, materialize};
use crate::format::{Action, Answer, Outcome, Record, Repository, Scenario};
use crate::resolve::{entries, entry_name, installed};

/// A second resolver, run as a command.
#[derive(Debug, Clone)]
pub struct Other {
    argv: Vec<String>,
}

impl Other {
    /// The command `argv`, to which the scenario's directory is appended.
    pub fn new(argv: Vec<String>) -> Other {
        Other { argv }
    }

    /// The command's answer to the scenario laid out in `directory`, or
    /// `None` where it failed.
    pub fn answer_in(&self, directory: &std::path::Path) -> io::Result<Option<Answer>> {
        let (program, arguments) = self
            .argv
            .split_first()
            .ok_or_else(|| io::Error::other("no command for the second resolver"))?;
        let output = Command::new(program)
            .args(arguments)
            .arg(directory)
            .output()?;
        if !output.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Answer::parse_lines("the second resolver", &text)
            .map(Some)
            .map_err(io::Error::other)
    }
}

/// What of two answers is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    /// The packages each step concerns and what it does to them.
    Names,
    /// The steps, versions included, in any order.
    Set,
    /// The steps in order.
    Sequence,
}

/// Whether `apk` and `other` differ in what `compare` looks at.
fn differ(apk: &Answer, other: &Answer, compare: Compare) -> bool {
    match compare {
        Compare::Names => concerned(apk) != concerned(other),
        Compare::Set => !apk.same_steps(other),
        Compare::Sequence => apk.outcome != other.outcome,
    }
}

/// What each step of `answer` does to which package, or `None` where it
/// refuses.
fn concerned(answer: &Answer) -> Option<BTreeSet<(Action, &str)>> {
    match &answer.outcome {
        Outcome::Changes(steps) => Some(
            steps
                .iter()
                .map(|step| (step.action, step.name.as_str()))
                .collect(),
        ),
        Outcome::Refused => None,
    }
}

fn names(answer: &Answer) -> BTreeSet<String> {
    match &answer.outcome {
        Outcome::Changes(steps) => steps.iter().map(|step| step.name.clone()).collect(),
        Outcome::Refused => BTreeSet::new(),
    }
}

/// The shape of the disagreement a reduction keeps.
#[derive(Debug)]
struct Signature {
    compare: Compare,
    requested: bool,
    apk_refused: bool,
    other_refused: bool,
    /// Whether both sides make the same steps, in another order.
    same_steps: bool,
    only_apk: BTreeSet<String>,
    only_other: BTreeSet<String>,
    /// Where the sequences first part, comparing sequences.
    parting: Option<Parting>,
}

impl Signature {
    /// The disagreement between `apk` and `other`, compared as `compare`
    /// says, with the packages named in `keep` on the side each is on.
    ///
    /// # Errors
    ///
    /// Fails where the two do not disagree, or a package named in `keep` is
    /// on both sides or neither.
    fn of(
        apk: &Answer,
        other: &Answer,
        compare: Compare,
        keep: &[String],
        requested: bool,
    ) -> Result<Signature, String> {
        if !differ(apk, other, compare) {
            return Err("apk and the second resolver agree".into());
        }
        let (apk_names, other_names) = (names(apk), names(other));
        let mut signature = Signature {
            compare,
            requested,
            apk_refused: apk.outcome == Outcome::Refused,
            other_refused: other.outcome == Outcome::Refused,
            same_steps: apk.same_steps(other),
            only_apk: BTreeSet::new(),
            only_other: BTreeSet::new(),
            parting: (compare == Compare::Sequence)
                .then(|| parting(apk, other))
                .flatten(),
        };
        for name in keep {
            if apk_names.contains(name) && !other_names.contains(name) {
                signature.only_apk.insert(name.clone());
            } else if other_names.contains(name) && !apk_names.contains(name) {
                signature.only_other.insert(name.clone());
            } else {
                return Err(format!("{name} is not on one side of the disagreement"));
            }
        }
        Ok(signature)
    }

    /// Whether `apk` and `other` still disagree in this way.
    fn kept_by(&self, apk: &Answer, other: &Answer) -> bool {
        let refused = |answer: &Answer| answer.outcome == Outcome::Refused;
        if refused(apk) != self.apk_refused || refused(other) != self.other_refused {
            return false;
        }
        if !differ(apk, other, self.compare) || apk.same_steps(other) != self.same_steps {
            return false;
        }
        if self.parting.is_some() && parting(apk, other) != self.parting {
            return false;
        }
        let (apk, other) = (names(apk), names(other));
        self.only_apk
            .iter()
            .all(|name| apk.contains(name) && !other.contains(name))
            && self
                .only_other
                .iter()
                .all(|name| other.contains(name) && !apk.contains(name))
    }
}

/// Where two answers' sequences first part: what each does at the first
/// step they differ at, as `action name`, empty where one has run out.
type Parting = (String, String);

/// Where the steps of `apk` and `other` first part, if they do.
fn parting(apk: &Answer, other: &Answer) -> Option<Parting> {
    let steps = |answer: &Answer| match &answer.outcome {
        Outcome::Changes(steps) => steps
            .iter()
            .map(|step| format!("{:?} {}", step.action, step.name))
            .collect(),
        Outcome::Refused => Vec::new(),
    };
    let (ours, theirs): (Vec<String>, Vec<String>) = (steps(apk), steps(other));
    let at = (0..ours.len().max(theirs.len())).find(|&at| ours.get(at) != theirs.get(at))?;
    Some((
        ours.get(at).cloned().unwrap_or_default(),
        theirs.get(at).cloned().unwrap_or_default(),
    ))
}

/// One removable part of a scenario.
///
/// A record's lines and entries are named by the list of records holding
/// it, as [`lists`] numbers them, and its place in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    /// A record: its repository and its place in it.
    Record(usize, usize),
    /// A field line of a record.
    Line(usize, usize, usize),
    /// An entry of a list field of a record.
    Entry(usize, usize, usize, usize),
    /// An installed record.
    Installed(usize),
    /// A world entry.
    World(usize),
    /// A request entry.
    Request(usize),
}

/// The letters of the fields an entry list is read from.
const LISTS: [char; 3] = ['D', 'p', 'i'];

/// The letters of the fields the line pass leaves in place: those that name
/// and identify a package.
const KEPT: [char; 4] = ['C', 'P', 'V', 'A'];

/// Each list of records `scenario` holds, numbered as a [`Part`] names it:
/// each repository of records by its place among the repositories, and the
/// installed set after them all.
fn lists(scenario: &Scenario) -> Vec<(usize, &[Record])> {
    let mut lists: Vec<(usize, &[Record])> = scenario
        .repositories
        .iter()
        .enumerate()
        .filter_map(|(at, repository)| match repository {
            Repository::Records(records) => Some((at, records.as_slice())),
            Repository::Verbatim(_) => None,
        })
        .collect();
    lists.push((scenario.repositories.len(), &scenario.installed));
    lists
}

/// The parts of `scenario` of the kind `example` is.
fn parts(scenario: &Scenario, example: Part) -> Vec<Part> {
    let mut parts = Vec::new();
    let installed = scenario.repositories.len();
    for (repository, records) in lists(scenario) {
        for (at, record) in records.iter().enumerate() {
            match example {
                // An installed record is removed whole as `Installed`.
                Part::Record(..) if repository != installed => {
                    parts.push(Part::Record(repository, at))
                }
                Part::Line(..) => {
                    for (line, (letter, _)) in record.fields.iter().enumerate() {
                        if !KEPT.contains(letter) {
                            parts.push(Part::Line(repository, at, line));
                        }
                    }
                }
                Part::Entry(..) => {
                    for (line, (letter, value)) in record.fields.iter().enumerate() {
                        if LISTS.contains(letter) {
                            for entry in 0..value.split(' ').filter(|e| !e.is_empty()).count() {
                                parts.push(Part::Entry(repository, at, line, entry));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    match example {
        Part::Installed(_) => parts.extend((0..scenario.installed.len()).map(Part::Installed)),
        Part::World(_) => parts.extend((0..scenario.world.len()).map(Part::World)),
        Part::Request(_) => parts.extend((0..scenario.request.len()).map(Part::Request)),
        _ => {}
    }
    parts
}

/// The record `record`, the `at`-th of the list numbered `list`, without
/// the lines and entries in `removed`.
fn trimmed(record: &Record, list: usize, at: usize, removed: &BTreeSet<Part>) -> Record {
    let mut fields = Vec::new();
    for (line, (letter, value)) in record.fields.iter().enumerate() {
        if removed.contains(&Part::Line(list, at, line)) {
            continue;
        }
        if !LISTS.contains(letter) {
            fields.push((*letter, value.clone()));
            continue;
        }
        let entries: Vec<&str> = value
            .split(' ')
            .filter(|entry| !entry.is_empty())
            .enumerate()
            .filter(|(entry, _)| !removed.contains(&Part::Entry(list, at, line, *entry)))
            .map(|(_, entry)| entry)
            .collect();
        if !entries.is_empty() || value.is_empty() {
            fields.push((*letter, entries.join(" ")));
        }
    }
    Record { fields }
}

/// `scenario` without the parts in `removed`.
fn without(scenario: &Scenario, removed: &BTreeSet<Part>) -> Scenario {
    let mut reduced = scenario.clone();
    for (repository, records) in reduced.repositories.iter_mut().enumerate() {
        let Repository::Records(records) = records else {
            continue;
        };
        *records = records
            .iter()
            .enumerate()
            .filter(|(at, _)| !removed.contains(&Part::Record(repository, *at)))
            .map(|(at, record)| trimmed(record, repository, at, removed))
            .collect();
    }
    let installed = scenario.repositories.len();
    reduced.installed = scenario
        .installed
        .iter()
        .enumerate()
        .filter(|(at, _)| !removed.contains(&Part::Installed(*at)))
        .map(|(at, record)| trimmed(record, installed, at, removed))
        .collect();
    let keep = |part: Part| !removed.contains(&part);
    reduced.world = enumerate_kept(&scenario.world, |at| keep(Part::World(at)));
    reduced.request = enumerate_kept(&scenario.request, |at| keep(Part::Request(at)));
    reduced
}

fn enumerate_kept<T: Clone>(items: &[T], keep: impl Fn(usize) -> bool) -> Vec<T> {
    items
        .iter()
        .enumerate()
        .filter(|(at, _)| keep(*at))
        .map(|(_, item)| item.clone())
        .collect()
}

/// A reduction in progress.
pub struct Reducer<'a> {
    apk: &'a Apk,
    other: &'a Other,
    compare: Compare,
    keep: Vec<String>,
    consistent: bool,
    signature: Option<Signature>,
    /// How many scenarios have been run.
    pub tests: usize,
    /// How many of them the second resolver failed on.
    pub failures: usize,
    log: Box<dyn FnMut(&str) + 'a>,
}

impl<'a> Reducer<'a> {
    /// A reducer comparing `apk` with `other`, keeping the packages named in
    /// `keep` on the side of the disagreement they start on, and reporting
    /// its progress to `log`.
    pub fn new(
        apk: &'a Apk,
        other: &'a Other,
        compare: Compare,
        keep: Vec<String>,
        log: impl FnMut(&str) + 'a,
    ) -> Reducer<'a> {
        Reducer {
            apk,
            other,
            compare,
            keep,
            consistent: false,
            signature: None,
            tests: 0,
            failures: 0,
            log: Box::new(log),
        }
    }

    /// The reducer, keeping the root consistent where `consistent` holds:
    /// it then removes nothing that leaves the root as apk would not have
    /// made it (see the [module documentation](self)), so that what it
    /// keeps is a request over a root apk could have installed.
    pub fn consistent(mut self, consistent: bool) -> Reducer<'a> {
        self.consistent = consistent;
        self
    }

    /// Both answers to `scenario`, the second `None` where it failed.
    fn answers(&mut self, scenario: &Scenario) -> io::Result<(Answer, Option<Answer>)> {
        self.tests += 1;
        let directory = TempDir::new()?;
        materialize(scenario, directory.path())?;
        let apk = self.apk.answer_in(scenario, directory.path())?;
        let other = self.other.answer_in(directory.path())?;
        if other.is_none() {
            self.failures += 1;
        }
        Ok((apk, other))
    }

    /// Whether `scenario` shows the disagreement the reduction keeps.
    fn interesting(&mut self, scenario: &Scenario) -> io::Result<bool> {
        if self.consistent && !consistent(scenario) {
            return Ok(false);
        }
        if self
            .signature
            .as_ref()
            .is_some_and(|signature| signature.requested && scenario.request.is_empty())
        {
            return Ok(false);
        }
        let (apk, Some(other)) = self.answers(scenario)? else {
            return Ok(false);
        };
        Ok(self
            .signature
            .as_ref()
            .is_some_and(|signature| signature.kept_by(&apk, &other)))
    }

    /// Reduces `scenario`, which must show a disagreement to begin with,
    /// and a consistent root where the reducer keeps one.
    pub fn reduce(&mut self, scenario: &Scenario) -> io::Result<Scenario> {
        if self.consistent && !consistent(scenario) {
            return Err(io::Error::other(
                "the root is not one apk could have made to begin with",
            ));
        }
        let (apk, other) = self.answers(scenario)?;
        let other = other.ok_or_else(|| io::Error::other("the second resolver failed"))?;
        let signature = Signature::of(
            &apk,
            &other,
            self.compare,
            &self.keep,
            !scenario.request.is_empty(),
        )
        .map_err(io::Error::other)?;
        self.signature = Some(signature);

        let mut current = scenario.clone();
        let pruned = prune(&current);
        if pruned != current && self.interesting(&pruned)? {
            current = pruned;
        }
        (self.log)(&format!(
            "{} records to begin with",
            count_records(&current)
        ));
        loop {
            let before = current.clone();
            for example in [
                Part::Record(0, 0),
                Part::Line(0, 0, 0),
                Part::Entry(0, 0, 0, 0),
                Part::Installed(0),
                Part::World(0),
                Part::Request(0),
            ] {
                current = self.pass(current, example)?;
            }
            (self.log)(&format!(
                "round done: {} records, {} tests",
                count_records(&current),
                self.tests
            ));
            if current == before {
                return Ok(current);
            }
        }
    }

    /// Removes what it can of one kind of part: sweeps the parts in chunks,
    /// removing each chunk the disagreement survives without and moving on
    /// past each it does not, then halves the chunks, down to single parts,
    /// and sweeps single parts until a sweep removes nothing.
    fn pass(&mut self, scenario: Scenario, example: Part) -> io::Result<Scenario> {
        let all = parts(&scenario, example);
        let mut kept = all.clone();
        let mut size = (kept.len() / 2).max(1);
        while !kept.is_empty() {
            let mut changed = false;
            let mut at = 0;
            while at < kept.len() {
                let end = (at + size).min(kept.len());
                let candidate: Vec<Part> = kept[..at].iter().chain(&kept[end..]).copied().collect();
                let kept_set: BTreeSet<Part> = candidate.iter().copied().collect();
                let removed: BTreeSet<Part> = all
                    .iter()
                    .filter(|part| !kept_set.contains(part))
                    .copied()
                    .collect();
                if self.interesting(&without(&scenario, &removed))? {
                    kept = candidate;
                    changed = true;
                } else {
                    at = end;
                }
            }
            if size == 1 {
                if !changed {
                    break;
                }
            } else {
                size = (size / 2).max(1);
            }
        }
        let kept: BTreeSet<Part> = kept.into_iter().collect();
        let removed: BTreeSet<Part> = all
            .iter()
            .filter(|part| !kept.contains(part))
            .copied()
            .collect();
        if !removed.is_empty() {
            (self.log)(&format!(
                "removed {} parts of the kind {example:?}, {} tests so far",
                removed.len(),
                self.tests
            ));
        }
        Ok(without(&scenario, &removed))
    }
}

/// Whether `scenario`'s root is one apk could have made: each installed
/// package's dependencies answered by installed packages, none forbidden by
/// another installed package's `!` entry, and each world entry answered by
/// an installed package, or for a `!` entry, forbidding none. A package's
/// own `!` entry does not reach it: apk installs a package that conflicts
/// with a name it provides (the behavior catalog's
/// `conflicts/a-package-may-conflict-with-a-name-it-provides`). Whether an
/// entry admits a package is the library's rule, [`Record::answers`]. A
/// root ferrata cannot read is not one.
///
/// [`Record::answers`]: ferrata::record::Record::answers
fn consistent(scenario: &Scenario) -> bool {
    let Ok(installed) = installed(scenario) else {
        return false;
    };
    let Ok(world) = entries(&scenario.world) else {
        return false;
    };
    // Whether `entry`, stated by the installed package numbered `stating`
    // or by the world, is met.
    let met = |entry: &Dependency, stating: Option<usize>| {
        let mut others = installed
            .iter()
            .enumerate()
            .filter(|&(at, _)| Some(at) != stating);
        if entry.is_conflict() {
            !others.any(|(_, record)| record.answers(entry))
        } else {
            installed.iter().any(|record| record.answers(entry))
        }
    };
    installed
        .iter()
        .enumerate()
        .all(|(at, record)| record.depends().iter().all(|entry| met(entry, Some(at))))
        && world.iter().all(|entry| met(entry, None))
}

fn count_records(scenario: &Scenario) -> usize {
    scenario
        .repositories
        .iter()
        .map(|repository| match repository {
            Repository::Records(records) => records.len(),
            Repository::Verbatim(_) => 0,
        })
        .sum()
}

/// `scenario` without the records no resolution of its request could
/// reach: those that neither carry nor provide a name reachable from the
/// request and the world, through every record's dependencies, conflicts and
/// provides, and whose `install_if` names no reachable package (a `!` entry
/// in it, which asks for a package's absence, is not counted). apk weighs a
/// package whose `install_if` could hold even where it does not install it.
pub fn prune(scenario: &Scenario) -> Scenario {
    let records: Vec<&Record> = scenario
        .repositories
        .iter()
        .filter_map(|repository| match repository {
            Repository::Records(records) => Some(records.iter()),
            Repository::Verbatim(_) => None,
        })
        .flatten()
        .chain(&scenario.installed)
        .collect();
    let list = |record: &Record, letter: char| -> Vec<String> {
        record
            .fields
            .iter()
            .filter(|(field, _)| *field == letter)
            .flat_map(|(_, value)| value.split(' ').filter(|e| !e.is_empty()))
            .filter_map(entry_name)
            .collect()
    };
    // The packages an `install_if` asks to be present: its `!` entries ask
    // for absence instead, which any resolution can grant.
    let condition = |record: &Record| -> Vec<String> {
        record
            .fields
            .iter()
            .filter(|(field, _)| *field == 'i')
            .flat_map(|(_, value)| {
                value
                    .split(' ')
                    .filter(|e| !e.is_empty() && !e.starts_with('!'))
            })
            .filter_map(entry_name)
            .collect()
    };
    let carries = |record: &Record, reachable: &BTreeSet<String>| {
        reachable.contains(record.name())
            || list(record, 'p')
                .iter()
                .any(|name| reachable.contains(name))
    };
    let mut reachable: BTreeSet<String> = scenario
        .request
        .iter()
        .chain(&scenario.world)
        .filter_map(|entry| entry_name(entry))
        .collect();
    loop {
        let before = reachable.len();
        for record in &records {
            let triggered = condition(record)
                .iter()
                .any(|name| reachable.contains(name));
            if carries(record, &reachable) || triggered {
                reachable.insert(record.name().to_owned());
                reachable.extend(list(record, 'D'));
                reachable.extend(list(record, 'p'));
            }
        }
        if reachable.len() == before {
            break;
        }
    }
    let mut pruned = scenario.clone();
    for repository in &mut pruned.repositories {
        if let Repository::Records(records) = repository {
            records.retain(|record| {
                carries(record, &reachable)
                    || condition(record)
                        .iter()
                        .any(|name| reachable.contains(name))
            });
        }
    }
    pruned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Step;

    fn record(fields: &[(char, &str)]) -> Record {
        Record {
            fields: fields.iter().map(|(l, v)| (*l, (*v).to_owned())).collect(),
        }
    }

    #[test]
    fn removing_entries_keeps_the_rest_of_a_field() {
        let scenario = Scenario {
            repositories: vec![Repository::Records(vec![record(&[
                ('P', "a"),
                ('V', "1"),
                ('D', "x y z"),
            ])])],
            request: vec!["a".into()],
            ..Scenario::default()
        };
        let removed = BTreeSet::from([Part::Entry(0, 0, 2, 1)]);
        let reduced = without(&scenario, &removed);
        let Repository::Records(records) = &reduced.repositories[0] else {
            panic!()
        };
        assert_eq!(records[0].get('D'), Some("x z"));
        let removed = BTreeSet::from([
            Part::Entry(0, 0, 2, 0),
            Part::Entry(0, 0, 2, 1),
            Part::Entry(0, 0, 2, 2),
        ]);
        let reduced = without(&scenario, &removed);
        let Repository::Records(records) = &reduced.repositories[0] else {
            panic!()
        };
        assert_eq!(records[0].get('D'), None);
    }

    #[test]
    fn an_installed_record_loses_lines_and_entries_as_a_repository_s_does() {
        let scenario = Scenario {
            repositories: vec![Repository::Records(vec![record(&[('P', "a"), ('V', "2")])])],
            installed: vec![
                record(&[('P', "a"), ('V', "1"), ('D', "x y"), ('S', "10")]),
                record(&[('P', "b"), ('V', "1")]),
            ],
            request: vec!["a".into()],
            ..Scenario::default()
        };
        let lines = parts(&scenario, Part::Line(0, 0, 0));
        assert_eq!(lines, [Part::Line(1, 0, 2), Part::Line(1, 0, 3)]);
        assert_eq!(parts(&scenario, Part::Record(0, 0)), [Part::Record(0, 0)]);
        assert_eq!(
            parts(&scenario, Part::Installed(0)),
            [Part::Installed(0), Part::Installed(1)]
        );
        let removed = BTreeSet::from([Part::Line(1, 0, 3), Part::Entry(1, 0, 2, 0)]);
        let reduced = without(&scenario, &removed);
        assert_eq!(
            reduced.installed[0],
            record(&[('P', "a"), ('V', "1"), ('D', "y")])
        );
        assert_eq!(reduced.installed[1], scenario.installed[1]);
        assert_eq!(reduced.repositories, scenario.repositories);
    }

    #[test]
    fn sequences_part_where_they_first_differ() {
        let answer = |steps: &[(Action, &str)]| Answer {
            apk: "apk".into(),
            messages: Vec::new(),
            outcome: Outcome::Changes(
                steps
                    .iter()
                    .map(|(action, name)| Step {
                        action: *action,
                        name: (*name).into(),
                        from: None,
                        version: "1".into(),
                    })
                    .collect(),
            ),
        };
        let apk = answer(&[(Action::Remove, "a"), (Action::Install, "b")]);
        let other = answer(&[(Action::Install, "b"), (Action::Remove, "a")]);
        assert_eq!(
            parting(&apk, &other),
            Some(("Remove a".to_owned(), "Install b".to_owned()))
        );
        assert_eq!(parting(&apk, &apk), None);
        let shorter = answer(&[(Action::Remove, "a")]);
        assert_eq!(
            parting(&apk, &shorter),
            Some(("Install b".to_owned(), String::new()))
        );
    }

    /// An answer installing `names` at version 1, in order.
    fn installs(names: &[&str]) -> Answer {
        Answer {
            apk: "apk".into(),
            messages: Vec::new(),
            outcome: Outcome::Changes(
                names
                    .iter()
                    .map(|name| Step {
                        action: Action::Install,
                        name: (*name).into(),
                        from: None,
                        version: "1".into(),
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn an_order_disagreement_is_not_reduced_to_one_of_what_changes() {
        let (ab, ba, b) = (
            installs(&["a", "b"]),
            installs(&["b", "a"]),
            installs(&["b"]),
        );
        let order = Signature::of(&ab, &ba, Compare::Sequence, &[], true).unwrap();
        assert!(order.kept_by(&ab, &ba));
        // `[a, b]` against `[b]` parts where `[a, b]` against `[b, a]` does,
        // and leaves `a` out.
        assert_eq!(parting(&ab, &b), parting(&ab, &ba));
        assert!(!order.kept_by(&ab, &b));
        // A disagreement about what changes, compared as sequences, stays
        // one.
        let set = Signature::of(&ab, &b, Compare::Sequence, &[], true).unwrap();
        assert!(set.kept_by(&ab, &b));
        assert!(!set.kept_by(&ab, &ba));
        assert!(Signature::of(&ab, &ab, Compare::Sequence, &[], true).is_err());
        assert!(Signature::of(&ab, &ba, Compare::Set, &[], true).is_err());
    }

    #[test]
    fn a_consistent_root_meets_its_dependencies_conflicts_and_world() {
        let root = |installed: Vec<Record>, world: &[&str]| Scenario {
            installed,
            world: world.iter().map(|entry| (*entry).to_owned()).collect(),
            ..Scenario::default()
        };
        let a = record(&[('P', "a"), ('V', "1"), ('D', "so:b !c")]);
        let b = record(&[('P', "b"), ('V', "1"), ('p', "so:b=1")]);
        let c = record(&[('P', "c"), ('V', "1")]);
        assert!(consistent(&root(vec![a.clone(), b.clone()], &["a"])));
        assert!(!consistent(&root(vec![a.clone()], &["a"])));
        assert!(!consistent(&root(vec![a.clone(), b.clone(), c], &["a"])));
        assert!(!consistent(&root(vec![a.clone(), b.clone()], &["a", "d"])));
        assert!(consistent(&root(vec![a.clone(), b.clone()], &["a", "!c"])));
        assert!(!consistent(&root(vec![a, b], &["a", "!b"])));
    }

    #[test]
    fn a_package_s_own_conflict_does_not_reach_it() {
        let root = |installed: Vec<Record>| Scenario {
            installed,
            world: vec!["p".into()],
            ..Scenario::default()
        };
        // apk installs a package forbidding a name it provides.
        let p = record(&[('P', "p"), ('V', "1"), ('p', "x=1"), ('D', "!x")]);
        assert!(consistent(&root(vec![p.clone()])));
        // Another package providing the name is still forbidden.
        let other = record(&[('P', "o"), ('V', "1"), ('p', "x=1")]);
        assert!(!consistent(&root(vec![p, other])));
    }

    #[test]
    fn pruning_keeps_what_the_request_can_reach() {
        let scenario = Scenario {
            repositories: vec![Repository::Records(vec![
                record(&[('P', "app"), ('V', "1"), ('D', "lib !old so:x")]),
                record(&[('P', "lib"), ('V', "1")]),
                record(&[('P', "old"), ('V', "1")]),
                record(&[('P', "libx"), ('V', "1"), ('p', "so:x=1")]),
                record(&[('P', "unrelated"), ('V', "1")]),
                record(&[('P', "plugin"), ('V', "1"), ('i', "app lib")]),
                record(&[('P', "other-plugin"), ('V', "1"), ('i', "app unrelated")]),
                record(&[('P', "unless"), ('V', "1"), ('i', "app !unrelated")]),
            ])],
            request: vec!["app".into()],
            ..Scenario::default()
        };
        let pruned = prune(&scenario);
        let Repository::Records(records) = &pruned.repositories[0] else {
            panic!()
        };
        let names: Vec<&str> = records.iter().map(Record::name).collect();
        assert_eq!(
            names,
            [
                "app",
                "lib",
                "old",
                "libx",
                "plugin",
                "other-plugin",
                "unless"
            ]
        );
    }
}
