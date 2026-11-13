//! The behavior catalog's own checks.
//!
//! Every run checks that each scenario reads, writes back as itself, states
//! its rule, holds records ferrata reads, its installed records among them,
//! and carries an answer from the apk release the dialect was measured
//! against; that ferrata answers every scenario as apk did, a scenario it
//! cannot answer counting as one it answers otherwise, but for those holding
//! what ferrata does not act on yet and the known differences the
//! `resolve` module documents ([`known_differences`]); and that every answer ferrata gives explains itself, as
//! [`properties::explained`] checks. With
//! `FERRATA_APK_STATIC` naming an `apk.static`, `apk_still_answers_so` asks
//! it again and fails on any answer that has changed.

use std::fs;
use std::path::{Path, PathBuf};

use ferrata::Dialect;
use ferrata_scenario::apk::{Apk, completed, records_text};
use ferrata_scenario::differential::agree;
use ferrata_scenario::format::{Outcome, Repository, Scenario};
use ferrata_scenario::{properties, resolve};

fn catalog() -> Vec<(PathBuf, Scenario)> {
    fn walk(directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, files);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "scenario")
            {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../catalog"),
        &mut files,
    );
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap();
            let scenario = Scenario::parse(&text)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            (path, scenario)
        })
        .collect()
}

#[test]
fn every_scenario_is_well_formed() {
    let catalog = catalog();
    let measured = format!("apk-tools {}-", Dialect::V3_0.measured_against());
    for (path, scenario) in &catalog {
        let path = path.display();
        assert_eq!(
            Scenario::parse(&scenario.to_string()).as_ref(),
            Ok(scenario),
            "{path} does not write back as itself"
        );
        assert!(!scenario.statement.is_empty(), "{path} states no rule");
        assert!(
            !scenario.repositories.is_empty(),
            "{path} has no repository"
        );
        assert!(!scenario.request.is_empty(), "{path} requests nothing");
        let answer = scenario
            .answer
            .as_ref()
            .unwrap_or_else(|| panic!("{path} has no answer"));
        assert!(
            answer.apk.starts_with(&measured),
            "{path} was answered by {}, not the release the dialect was measured against",
            answer.apk
        );
        // Records written as records are ones ferrata reads without error;
        // a verbatim repository is where a scenario about text apk cannot
        // read belongs. Installed records have no verbatim form, so every
        // one reads.
        let completed = completed(scenario);
        for repository in &completed.repositories {
            if let Repository::Records(records) = repository {
                let text = records_text(records);
                for item in ferrata::index::read(Dialect::V3_0, &text) {
                    item.unwrap_or_else(|error| panic!("{path}: {error}"));
                }
            }
        }
        resolve::installed(scenario).unwrap_or_else(|error| panic!("{path}: {error}"));
    }
    assert!(catalog.len() >= 40, "{} scenarios", catalog.len());
}

#[test]
fn apk_still_answers_so() {
    let Some(apk) = Apk::from_env() else {
        eprintln!("FERRATA_APK_STATIC is not set; nothing compared");
        return;
    };
    let apk = apk.unwrap();
    let mut changed = Vec::new();
    let catalog = catalog();
    for (path, scenario) in &catalog {
        let answer = apk.answer(scenario).unwrap();
        let recorded = scenario.answer.as_ref().unwrap();
        if !answer.same_answer(recorded) {
            changed.push(format!("{}:\n{}", path.display(), answer.lines()));
        }
    }
    eprintln!("{} scenarios asked of {}", catalog.len(), apk.banner());
    assert!(
        changed.is_empty(),
        "apk now answers otherwise:\n{}",
        changed.join("\n")
    );
}

/// The scenarios ferrata is known to answer otherwise than apk, as the
/// `resolve` module's documentation lists them under "Where ferrata answers
/// otherwise": every quoted catalog path there. The list is read from that
/// documentation rather than kept here too, so the two cannot part. A
/// scenario listed that ferrata comes to answer as apk does fails
/// [`ferrata_answers_as_apk_does`], so the list stays exact.
fn known_differences() -> Vec<&'static str> {
    const DOCUMENTATION: &str = include_str!("../../src/resolve.rs");
    let section: Vec<&str> = DOCUMENTATION
        .lines()
        .map_while(|line| line.strip_prefix("//!"))
        .skip_while(|line| line.trim() != "# Where ferrata answers otherwise")
        .skip(1)
        .take_while(|line| !line.starts_with(" # "))
        .collect();
    section
        .iter()
        .flat_map(|line| line.split('`').skip(1).step_by(2))
        .filter(|quoted| {
            quoted.contains('/')
                && quoted.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-/".contains(&byte)
                })
        })
        .collect()
}

/// Whether `path` is a scenario `known` lists.
fn is_known(known: &[&str], path: &Path) -> bool {
    let path = path.with_extension("");
    known.iter().any(|name| path.ends_with(name))
}

#[test]
fn known_differences_are_scenarios_of_the_catalog() {
    let known = known_differences();
    assert!(
        !known.is_empty(),
        "the resolve module documents no known difference"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../catalog");
    let missing: Vec<_> = known
        .iter()
        .filter(|name| !root.join(format!("{name}.scenario")).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "documented as known differences, but no scenario of the catalog: {missing:?}"
    );
}

#[test]
fn ferrata_answers_as_apk_does() {
    let catalog = catalog();
    let known = known_differences();
    let mut compared = 0;
    let mut wrong = Vec::new();
    let mut pending = Vec::new();
    for (path, scenario) in &catalog {
        let listed = is_known(&known, path);
        let reason = resolve::unsupported(scenario).or(listed.then_some("a known difference"));
        let apk = scenario.answer.as_ref().unwrap();
        // A scenario ferrata cannot answer is answered otherwise than apk
        // answers it.
        let ours = resolve::answer(scenario);
        match (ours, reason) {
            (Ok(ours), _) if agree(apk, &ours) && listed => wrong.push(format!(
                "{}: answered as apk does, and listed as a known difference",
                path.display()
            )),
            (Ok(ours), _) if agree(apk, &ours) => compared += 1,
            (Ok(_), Some(reason)) => pending.push(format!("{} ({reason})", path.display())),
            (Err(error), Some(reason)) => pending.push(format!(
                "{} ({reason}; not answered: {error})",
                path.display()
            )),
            (Ok(ours), None) => wrong.push(format!(
                "{}:\n  apk:     {}\n  ferrata: {}",
                path.display(),
                apk.lines().replace('\n', " ").trim_end(),
                ours.lines().replace('\n', " ").trim_end()
            )),
            (Err(error), None) => wrong.push(format!("{}: not answered: {error}", path.display())),
        }
    }
    eprintln!(
        "{compared} of {} scenarios answered as apk answers them; {} differ as known, or hold what ferrata does not act on yet:\n  {}",
        catalog.len(),
        pending.len(),
        pending.join("\n  ")
    );
    assert!(
        wrong.is_empty(),
        "ferrata answers otherwise:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn every_answer_explains_itself() {
    let catalog = catalog();
    let mut resolved = 0;
    let mut refused = 0;
    let mut unread = 0;
    let mut wrong = Vec::new();
    for (path, scenario) in &catalog {
        let checked = resolve::resolved(scenario, |answer, installed| {
            match answer {
                Ok(_) => resolved += 1,
                Err(_) => refused += 1,
            }
            properties::explained(answer, installed)
        });
        match checked {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(problem))) => wrong.push(format!("{}: {problem}", path.display())),
            // An entry of the request or the world that does not read is
            // refused before anything is resolved, so there is nothing to
            // explain, where apk refuses it too.
            Ok(Err(_))
                if scenario.answer.as_ref().map(|answer| &answer.outcome)
                    == Some(&Outcome::Refused) =>
            {
                unread += 1;
            }
            Ok(Err(answer)) => wrong.push(format!(
                "{}: refused before resolving, where apk answers:\n{}",
                path.display(),
                answer.lines()
            )),
            Err(error) => wrong.push(format!("{}: not answered: {error}", path.display())),
        }
    }
    eprintln!(
        "{resolved} resolutions and {refused} refusals explained; {unread} requests refused, as apk refuses them, for an entry that does not read"
    );
    assert!(
        wrong.is_empty(),
        "explanations break their properties:\n{}",
        wrong.join("\n")
    );
}
