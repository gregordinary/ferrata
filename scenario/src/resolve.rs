//! ferrata's answer to a scenario, written as apk's is, so the two compare.

use ferrata::dependency::Dependency;
use ferrata::index::{self, Located};
use ferrata::record::Record;
use ferrata::resolve::{Change, Refusal, Request, Resolution};
use ferrata::{Dialect, Resolver};

use crate::apk::{completed, records_text};
use crate::format::{Action, Answer, Outcome, Repository, Scenario, Step, printed};

/// What a scenario's answer names as having given it, for ferrata.
pub const BANNER: &str = "ferrata";

/// What in `scenario` ferrata does not act on yet, if anything: a scenario
/// that holds it may be answered otherwise than apk answers it.
pub fn unsupported(_scenario: &Scenario) -> Option<&'static str> {
    None
}

/// The records of each of `scenario`'s repositories, in order, as ferrata
/// reads them from the index apk is given: up to the first record that does
/// not read, since apk stops reading an index at its first format error and
/// keeps the records before it.
pub fn repositories(scenario: &Scenario) -> Vec<Vec<Record>> {
    completed(scenario)
        .repositories
        .iter()
        .map(|repository| {
            let text = match repository {
                Repository::Records(records) => records_text(records),
                Repository::Verbatim(text) => text.clone(),
            };
            index::read(Dialect::V3_0, &text)
                .map_while(Result::ok)
                .map(Located::into_record)
                .collect()
        })
        .collect()
}

/// The records of `scenario`'s installed set, as ferrata reads them from the
/// installed database apk is given.
///
/// # Errors
///
/// Fails where a record does not read.
pub fn installed(scenario: &Scenario) -> Result<Vec<Record>, String> {
    index::read_installed(Dialect::V3_0, &records_text(&completed(scenario).installed))
        .map(|located| located.map(Located::into_record))
        .collect::<Result<_, _>>()
        .map_err(|error| format!("the installed set: {error}"))
}

/// A resolver over `repositories`, in order, and the root that `installed`
/// and its `world` make, an empty one where both are empty.
pub fn resolver<'a>(
    repositories: &'a [Vec<Record>],
    installed: &'a [Record],
    world: &'a [Dependency],
) -> Resolver<'a> {
    repositories
        .iter()
        .fold(Resolver::new(Dialect::V3_0), Resolver::repository)
        .installed(installed, world)
}

/// The entries `lines` make, each an argument to `apk add` or a line of a
/// world.
///
/// # Errors
///
/// Fails with a message naming the first entry that does not read, which apk
/// refuses too.
pub fn entries(lines: &[impl AsRef<str>]) -> Result<Vec<Dependency>, String> {
    lines
        .iter()
        .map(|line| {
            let line = line.as_ref();
            Dialect::V3_0
                .dependency(line)
                .map_err(|error| format!("{line}: {error}"))
        })
        .collect()
}

/// The name `entry` is about, an entry of a list field, a request or a
/// world, as the library reads the entry: `None` where it does not read.
pub fn entry_name(entry: &str) -> Option<String> {
    let entry = Dialect::V3_0.dependency(entry).ok()?;
    Some(printed(entry.name().as_bytes()))
}

/// The request `lines` make, each an argument to `apk add`.
///
/// # Errors
///
/// Fails as [`entries`] does.
pub fn request(lines: &[impl AsRef<str>]) -> Result<Request, String> {
    entries(lines).map(|entries| entries.into_iter().collect())
}

/// ferrata's `answer`, written as a scenario's answer: the changes it makes
/// to the root, in order, or its refusal, with the refusal's reason as a
/// message, and each step of its explanation as one more.
pub fn written(answer: &Result<Resolution<'_>, Refusal>) -> Answer {
    match answer {
        Ok(resolution) => Answer {
            apk: BANNER.into(),
            messages: Vec::new(),
            outcome: Outcome::Changes(resolution.changes().iter().map(step).collect()),
        },
        Err(refusal) => refused(
            std::iter::once(refusal.to_string())
                .chain(
                    refusal
                        .explanation()
                        .steps()
                        .iter()
                        .map(|step| format!("  {step}")),
                )
                .collect(),
        ),
    }
}

/// Resolves `scenario`, and gives `then` ferrata's answer and the records of
/// the root's installed set; or, where the world or the request holds an
/// entry that does not read, which apk refuses too, gives the answer that
/// refuses it.
///
/// # Errors
///
/// Fails for a scenario whose installed set ferrata cannot read.
pub fn resolved<T>(
    scenario: &Scenario,
    then: impl FnOnce(&Result<Resolution<'_>, Refusal>, &[Record]) -> T,
) -> Result<Result<T, Answer>, String> {
    let repositories = repositories(scenario);
    let installed = installed(scenario)?;
    let completed = completed(scenario);
    let world = match entries(&completed.world) {
        Ok(world) => world,
        Err(message) => return Ok(Err(refused(vec![format!("world {message}")]))),
    };
    let request = match request(&completed.request) {
        Ok(request) => request,
        Err(message) => return Ok(Err(refused(vec![message]))),
    };
    let answer = resolver(&repositories, &installed, &world).resolve(&request);
    Ok(Ok(then(&answer, &installed)))
}

/// ferrata's answer to `scenario`, as [`written`] writes it.
///
/// # Errors
///
/// Fails for a scenario whose installed set ferrata cannot read.
pub fn answer(scenario: &Scenario) -> Result<Answer, String> {
    Ok(resolved(scenario, |answer, _| written(answer))?.unwrap_or_else(|refused| refused))
}

/// Why ferrata answers `scenario` as it does: for each package of the
/// resolution, and each package the root held that it drops, whose name is
/// one of `names`, or for every one where `names` is empty, its
/// explanation; or its refusal, and the refusal's explanation.
///
/// # Errors
///
/// Fails for a scenario whose installed set ferrata cannot read, and where
/// `names` names no package of the resolution or the root.
pub fn explanations(scenario: &Scenario, names: &[impl AsRef<str>]) -> Result<String, String> {
    let explained = resolved(scenario, |answer, installed| {
        let resolution = match answer {
            Ok(resolution) => resolution,
            Err(_) => return Ok(written(answer).lines()),
        };
        let wanted = |record: &Record| {
            names.is_empty() || names.iter().any(|name| name.as_ref() == record.name())
        };
        // The packages of the resolution, then those of the root it drops.
        let records = resolution
            .packages()
            .iter()
            .copied()
            .chain(
                installed
                    .iter()
                    .filter(|record| resolution.exclusion(record).is_some()),
            )
            .filter(|record| wanted(record));
        let explanations: Vec<String> = records
            .filter_map(|record| resolution.explain(record))
            .map(|explanation| format!("{explanation}\n"))
            .collect();
        if explanations.is_empty() {
            return Err("no package of the resolution or the root has that name".to_string());
        }
        Ok(explanations.join("\n"))
    })?;
    match explained {
        Ok(text) => text,
        Err(refused) => Ok(refused.lines()),
    }
}

/// A change, as an answer writes it: each name and version as apk prints
/// it ([`printed`]).
fn step(change: &Change<'_>) -> Step {
    let (action, from, to) = match *change {
        Change::Install { package, .. } => (Action::Install, None, package),
        Change::Upgrade { from, to, .. } => (Action::Upgrade, Some(from), to),
        Change::Downgrade { from, to, .. } => (Action::Downgrade, Some(from), to),
        Change::Replace { from, to, .. } => (Action::Replace, Some(from), to),
        Change::Remove(record) => (Action::Remove, None, record),
    };
    Step {
        action,
        name: printed(to.name().as_bytes()),
        from: from.map(|from| printed(from.version().as_bytes())),
        version: printed(to.version().as_bytes()),
    }
}

fn refused(messages: Vec<String>) -> Answer {
    Answer {
        apk: BANNER.into(),
        messages,
        outcome: Outcome::Refused,
    }
}
