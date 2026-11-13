//! What every resolution holds, whatever apk would answer: checks that need
//! no apk.
//!
//! [`check`] resolves a request and holds the answer to these:
//!
//! - **The same input gives the same answer.** A request asked twice, and
//!   asked of a second resolver built from the same records, is answered the
//!   same, and so is ordering any set of records. One resolver asked many
//!   requests in turn answers each as a resolver built for it alone does
//!   ([`check_reuse`]).
//! - **Requirements are met.** Each requested name, and each dependency of
//!   each package installed, is answered inside the resolution, and every
//!   package installed is one apk can install, or was installed already.
//! - **No `!` entry is violated.** No package installed is forbidden by a
//!   `!` entry of another package installed, or of the request.
//! - **One package to a name provided at a version.** A name one package
//!   carries, or provides at a version, no other package installed carries
//!   or provides in any form.
//! - **Nothing unasked for.** Each package installed is reached from the
//!   request through dependencies, or is an `install_if` package whose
//!   condition the resolution meets.
//! - **Install order.** Each package is installed after a package answering
//!   each of its dependencies, unless one of those leads back to it, through
//!   dependencies, `!` entries and the conditions of packages, installed or
//!   not, all of which apk's walk follows: a cycle, broken where apk breaks
//!   it.
//! - **Ordering any set places each record once.**
//! - **The changes take the root to the resolution.** Over an installed
//!   set, each package of one that the other lacks is installed, replaced
//!   or removed, once, and a package replaced gives way to one of its name.
//! - **Each package says where it comes from.** A package the root held,
//!   kept or dropped, comes from the root; any other from the first
//!   repository whose records hold it.
//! - **Every answer explains itself.** Each package of a resolution has a
//!   reason, and an explanation that starts with it, installed or kept as
//!   the root held it or not; a package chosen for a requirement carries or
//!   provides the name required, and answers the requirement. Each package
//!   a change removes or replaces has an exclusion, and an explanation that
//!   starts with it; a requirement that does not admit it does not answer
//!   it, and a `!` entry forbidding it does. In every
//!   explanation, a refusal's included, each step comes before the steps it
//!   follows from, so no chain of steps returns to a step; and a package's
//!   chain holds only packages installed or kept and names required, and
//!   ends at a requirement of the request or the world.
//!
//! Over an installed set, the world is resolved with the request, and the
//! checks hold the answer to both.
//!
//! The checks state again what it is for a package to answer an entry
//! ([`answers`]), so that they hold the resolver to that rule rather than to
//! its own reading of it. Two of the library's answers they take as given:
//!
//! - [`Record::is_installable`], whether apk can install a record: that is
//!   how the reader classifies the record, by its flaws and whether it
//!   conflicts with itself, not a choice of the resolver's, and what the
//!   check asks is that the resolver installs nothing so classified.
//! - [`Requirement::answered_by`], whether a requirement an explanation
//!   names admits a package: the explanation checks hold an explanation to
//!   the resolution it explains, by the rule the resolver followed. The
//!   rule stated again is laxer for a package stating a name twice, and
//!   would report such a package as dropped for a requirement it answers
//!   where the requirement admits only the mention the rule does not take.
//!
//! A violation is a defect in ferrata whatever apk answers, so the checks run
//! on every test run over generated repositories, and the `resolve` and
//! `generated` fuzz targets drive them further.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use ferrata::dependency::Dependency;
use ferrata::record::{Identity, Record};
use ferrata::resolve::{
    Change, Exclusion, Explanation, Package, Reason, Refusal, Request, RequiredBy, Requirement,
    Resolution, Source, Step, order,
};
use ferrata::{Dialect, Name};

use crate::resolve::resolver;

/// A second resolver, built apart from the first.
fn resolver_again<'a>(
    repositories: &'a [Vec<Record>],
    installed: &'a [Record],
    world: &'a [Dependency],
) -> ferrata::Resolver<'a> {
    resolver(repositories, installed, world)
}

/// Whether `record` answers `entry`: it carries the name, or provides it, as
/// the entry admits. A name provided at no version answers an entry that
/// states nothing but the name, or admits an earlier version.
///
/// This states the rule a second time, apart from `Record::answers`, which
/// the resolver's own answers rest on: the checks hold the resolver to the
/// rule, not to itself. It is laxer only for a package that states a name
/// twice, which it lets answer at either mention.
pub fn answers(record: &Record, entry: &Dependency) -> bool {
    if let Some(identity) = entry.identity() {
        return record.name() == entry.name()
            && Identity::read(identity).as_ref() == Some(record.identity());
    }
    (record.name() == entry.name() && entry.admits(record.version()))
        || record.provides().iter().any(|provide| {
            provide.name() == entry.name()
                && match provide.version() {
                    Some(version) => entry.admits(version),
                    None => entry.admits_no_version(),
                }
        })
}

/// Whether the `!` entry forbids `record`: whether the entry, without its
/// `!`, would answer it, which [`answers`] says, since the `!` plays no
/// part there.
pub fn forbids(entry: &Dependency, record: &Record) -> bool {
    answers(record, entry)
}

/// Whether `record` carries or provides `name`.
fn carries(record: &Record, name: &Name) -> bool {
    record.name() == name
        || record
            .provides()
            .iter()
            .any(|provide| provide.name() == name)
}

/// Resolves `request` over `repositories`, in order, into the root that
/// `installed` and its `world` make, an empty one where both are empty, and
/// checks every property of the answer.
///
/// # Errors
///
/// Fails with a description of the first property the answer breaks.
pub fn check(
    repositories: &[Vec<Record>],
    installed: &[Record],
    world: &[Dependency],
    request: &Request,
) -> Result<(), String> {
    let resolver = resolver(repositories, installed, world);
    let answer = resolver.resolve(request);
    if answer != resolver.resolve(request) {
        return Err("a request asked twice is answered differently".into());
    }
    if answer != resolver_again(repositories, installed, world).resolve(request) {
        return Err("a second resolver over the same records answers differently".into());
    }
    let records: Vec<&Record> = repositories.iter().flatten().chain(installed).collect();
    let placed = order(records.iter().copied(), request);
    if placed != order(records.iter().copied(), request) {
        return Err("a set ordered twice is ordered differently".into());
    }
    let mut sorted = placed;
    sorted.sort_unstable();
    if !sorted.iter().copied().eq(0..records.len()) {
        return Err("an order does not place each record once".into());
    }
    explained(&answer, installed)?;
    if let Ok(resolution) = &answer {
        // The world is resolved with the request, whose entry for a name
        // replaces the world's.
        let mut asked: Request = world.iter().cloned().collect();
        asked.extend(request.entries().cloned());
        check_resolution(resolution, installed, &asked)?;
        check_reached(resolution.packages(), &asked)?;
        check_order(resolution.packages(), &records)?;
        check_changes(resolution, installed)?;
        check_sources(resolution, repositories, installed)?;
    }
    Ok(())
}

/// Resolves each of `requests` in turn with one resolver over
/// `repositories`, into the root that `installed` and its `world` make, and
/// checks that each is answered as a resolver built for it alone answers
/// it: a request changes nothing a resolver holds.
///
/// # Errors
///
/// Fails naming the first request answered otherwise.
pub fn check_reuse(
    repositories: &[Vec<Record>],
    installed: &[Record],
    world: &[Dependency],
    requests: &[Request],
) -> Result<(), String> {
    let shared = resolver(repositories, installed, world);
    for request in requests {
        let fresh = resolver_again(repositories, installed, world).resolve(request);
        if shared.resolve(request) != fresh {
            let entries: Vec<String> = request.entries().map(ToString::to_string).collect();
            return Err(format!(
                "{} is answered otherwise by a resolver asked other requests first",
                entries.join(" ")
            ));
        }
    }
    Ok(())
}

/// Requirements met, no `!` entry violated, and one package to each name
/// held.
fn check_resolution(
    resolution: &Resolution<'_>,
    installed: &[Record],
    request: &Request,
) -> Result<(), String> {
    let packages = resolution.packages();
    // A package installed already that apk could not install may stay.
    for record in packages
        .iter()
        .filter(|record| !installed.iter().any(|old| same(old, record)))
    {
        if !record.is_installable() {
            return Err(format!("{}, which apk cannot install, is", record.name()));
        }
    }
    for entry in request.entries().filter(|entry| !entry.is_conflict()) {
        if !packages.iter().any(|record| answers(record, entry)) {
            return Err(format!("{entry}, requested, is not answered"));
        }
    }
    for record in packages {
        for entry in record.depends().iter().filter(|entry| !entry.is_conflict()) {
            if !packages.iter().any(|other| answers(other, entry)) {
                return Err(format!(
                    "{entry}, needed by {}, is not answered",
                    record.name()
                ));
            }
        }
    }
    for record in packages {
        for entry in record.depends().iter().filter(|entry| entry.is_conflict()) {
            let others = packages
                .iter()
                .filter(|other| !std::ptr::eq(**other, *record));
            for other in others {
                if forbids(entry, other) {
                    return Err(format!(
                        "{entry}, of {}, forbids {}, installed",
                        record.name(),
                        other.name()
                    ));
                }
            }
        }
    }
    for entry in request.entries().filter(|entry| entry.is_conflict()) {
        for record in packages {
            if forbids(entry, record) {
                return Err(format!(
                    "{entry}, in the request, forbids {}, installed",
                    record.name()
                ));
            }
        }
    }
    // Each name a package of the resolution holds, and each it provides at
    // no version, with the package.
    let mut held: Vec<(&Name, *const Record)> = Vec::new();
    let mut shared: Vec<(&Name, *const Record)> = Vec::new();
    for record in packages {
        held.push((record.name(), *record));
        for provide in record.provides() {
            match provide.version() {
                Some(_) => held.push((provide.name(), *record)),
                None => shared.push((provide.name(), *record)),
            }
        }
    }
    for &(name, holder) in &held {
        let others = held
            .iter()
            .chain(&shared)
            .any(|&(other, record)| other == name && !std::ptr::eq(record, holder));
        if others {
            return Err(format!("{name} is held, and provided by another"));
        }
    }
    Ok(())
}

/// Each package reached from the request through dependencies, or an
/// `install_if` package whose condition the packages installed meet.
///
/// Every package answering an entry counts as reached through it, and a
/// condition counts as met when every entry of it but its `!` entries is
/// answered, so the check passes over what it cannot tell apart rather
/// than report a package apk would install too.
fn check_reached(packages: &[&Record], request: &Request) -> Result<(), String> {
    let met = |record: &Record| {
        let condition = record.install_if();
        !condition.is_empty()
            && condition
                .iter()
                .filter(|entry| !entry.is_conflict())
                .all(|entry| packages.iter().any(|other| answers(other, entry)))
    };
    let mut reached = vec![false; packages.len()];
    let mut stack: Vec<usize> = Vec::new();
    let reach = |entry: &Dependency, stack: &mut Vec<usize>, reached: &mut Vec<bool>| {
        for (at, other) in packages.iter().enumerate() {
            if !reached[at] && answers(other, entry) {
                reached[at] = true;
                stack.push(at);
            }
        }
    };
    for entry in request.entries().filter(|entry| !entry.is_conflict()) {
        reach(entry, &mut stack, &mut reached);
    }
    for (at, record) in packages.iter().enumerate() {
        if met(record) {
            reached[at] = true;
            stack.push(at);
        }
    }
    while let Some(at) = stack.pop() {
        for entry in packages[at].depends().iter().filter(|e| !e.is_conflict()) {
            reach(entry, &mut stack, &mut reached);
        }
    }
    match reached.iter().position(|reached| !reached) {
        Some(at) => Err(format!(
            "{} is installed, and nothing asks for it",
            packages[at].name()
        )),
        None => Ok(()),
    }
}

/// Each package after a package answering each of its dependencies, or in a
/// cycle with one that does not.
fn check_order(packages: &[&Record], records: &[&Record]) -> Result<(), String> {
    // Where the walk can go from a package: what its dependencies lead to,
    // a `!` entry to the packages carrying or providing its name at whatever
    // version; and the packages carrying or providing a name that a package,
    // installed or not, carries or provides whose condition has an entry the
    // package answers, or a `!` entry naming a name of the package that does
    // not forbid it.
    let leads_to = |from: usize| -> Vec<usize> {
        let mut next: Vec<usize> = packages[from]
            .depends()
            .iter()
            .flat_map(|entry| {
                (0..packages.len()).filter(move |&other| {
                    if entry.is_conflict() {
                        carries(packages[other], entry.name())
                    } else {
                        answers(packages[other], entry)
                    }
                })
            })
            .collect();
        let conditions = records.iter().filter(|record| {
            record.install_if().iter().any(|entry| {
                if entry.is_conflict() {
                    carries(packages[from], entry.name()) && !forbids(entry, packages[from])
                } else {
                    answers(packages[from], entry)
                }
            })
        });
        for condition in conditions {
            let names = std::iter::once(condition.name())
                .chain(condition.provides().iter().map(|provide| provide.name()));
            for name in names {
                next.extend((0..packages.len()).filter(|&other| carries(packages[other], name)));
            }
        }
        next
    };
    let reaches = |from: usize, to: usize| -> bool {
        let mut seen = vec![false; packages.len()];
        let mut stack = vec![from];
        while let Some(at) = stack.pop() {
            if at == to {
                return true;
            }
            if !std::mem::replace(&mut seen[at], true) {
                stack.extend(leads_to(at));
            }
        }
        false
    };
    for (at, record) in packages.iter().enumerate() {
        for entry in record.depends().iter().filter(|entry| !entry.is_conflict()) {
            let answering: Vec<usize> = (0..packages.len())
                .filter(|&other| answers(packages[other], entry))
                .collect();
            if answering.is_empty() || answering.iter().any(|&other| other <= at) {
                continue;
            }
            if !answering.iter().any(|&other| reaches(other, at)) {
                return Err(format!(
                    "{} is installed before {entry}, its dependency, outside a cycle",
                    record.name()
                ));
            }
        }
    }
    Ok(())
}

/// Whether two records are one package: their identity, whatever their
/// names, as apk tells packages apart. The library answers this itself;
/// the checks state it again, as they state what it is for a package to
/// answer an entry, so that a change to the library's answer shows here as
/// a violation.
fn same(a: &Record, b: &Record) -> bool {
    a.identity() == b.identity()
}

/// Each package of the resolution, and each the root held that it drops,
/// comes from the root where the root held it, and otherwise from the first
/// repository holding it.
fn check_sources(
    resolution: &Resolution<'_>,
    repositories: &[Vec<Record>],
    installed: &[Record],
) -> Result<(), String> {
    let dropped = installed
        .iter()
        .filter(|old| !resolution.packages().iter().any(|record| same(old, record)));
    for record in resolution.packages().iter().copied().chain(dropped) {
        let expected = if installed.iter().any(|old| same(old, record)) {
            Source::Installed
        } else {
            let at = repositories
                .iter()
                .position(|records| records.iter().any(|offered| same(offered, record)))
                .ok_or_else(|| format!("{} comes from nowhere", record.name()))?;
            Source::Repository(at)
        };
        let source = resolution.source(record);
        if source != Some(expected) {
            return Err(format!(
                "{} comes from {source:?}, not {expected:?}",
                record.name()
            ));
        }
    }
    Ok(())
}

/// The changes take `installed` to the resolution's packages: each package
/// of one that the other lacks is installed, replaced or removed, once, and
/// a package replaced gives way to one of its name, at a later version for
/// an upgrade, an earlier for a downgrade, and the same for a replacement.
/// Each package a change installs comes from the repository it names, as
/// [`Resolution::source`] says.
fn check_changes(resolution: &Resolution<'_>, installed: &[Record]) -> Result<(), String> {
    let packages = resolution.packages();
    let mut gone: Vec<&Record> = Vec::new();
    let mut come: Vec<&Record> = Vec::new();
    for change in resolution.changes() {
        let installs = match *change {
            Change::Install {
                package,
                repository,
            } => Some((package, repository)),
            Change::Upgrade { to, repository, .. }
            | Change::Downgrade { to, repository, .. }
            | Change::Replace { to, repository, .. } => Some((to, repository)),
            Change::Remove(_) => None,
        };
        if let Some((package, repository)) = installs
            && resolution.source(package) != Some(Source::Repository(repository))
        {
            return Err(format!(
                "{change:?} names repository {repository}, but the package comes from {:?}",
                resolution.source(package)
            ));
        }
        match *change {
            Change::Install { package, .. } => come.push(package),
            Change::Remove(record) => gone.push(record),
            Change::Upgrade { from, to, .. }
            | Change::Downgrade { from, to, .. }
            | Change::Replace { from, to, .. } => {
                if from.name() != to.name() {
                    return Err(format!("{change:?} replaces a package of another name"));
                }
                let expected = match change {
                    Change::Upgrade { .. } => Ordering::Greater,
                    Change::Downgrade { .. } => Ordering::Less,
                    Change::Replace { .. } | Change::Install { .. } | Change::Remove(_) => {
                        Ordering::Equal
                    }
                };
                if to.version().cmp(from.version()) != expected {
                    return Err(format!("{change:?} goes the other way"));
                }
                gone.push(from);
                come.push(to);
            }
        }
    }
    for record in packages {
        let was = installed.iter().any(|old| same(old, record));
        let count = come.iter().filter(|new| same(new, record)).count();
        if count != usize::from(!was) {
            return Err(format!("{} comes {count} times", record.name()));
        }
    }
    for old in installed {
        let stays = packages.iter().any(|record| same(old, record));
        let count = gone.iter().filter(|record| same(old, record)).count();
        if count != usize::from(!stays) {
            return Err(format!("{} goes {count} times", old.name()));
        }
    }
    if !come
        .iter()
        .all(|new| packages.iter().any(|record| same(new, record)))
    {
        return Err("a package comes that the resolution does not hold".into());
    }
    if !gone
        .iter()
        .all(|old| installed.iter().any(|record| same(old, record)))
    {
        return Err("a package goes that was not installed".into());
    }
    Ok(())
}

/// Every answer explains itself: each package of a resolution, and each
/// package a change removes or replaces, has an explanation that starts with
/// it, and every explanation, a refusal's included, lists each step before
/// the steps it follows from, as the module documentation sets out.
///
/// # Errors
///
/// Fails with a description of the first property an explanation breaks.
pub fn explained(
    answer: &Result<Resolution<'_>, Refusal>,
    installed: &[Record],
) -> Result<(), String> {
    let resolution = match answer {
        Ok(resolution) => resolution,
        Err(refusal) => {
            let explanation = refusal.explanation();
            return check_steps(explanation, false)
                .map_err(|problem| format!("{refusal}: {problem}\n{explanation}"));
        }
    };
    for &record in resolution.packages() {
        let named = format!("{}-{}", record.name(), record.version());
        let kept = installed.iter().any(|old| same(old, record));
        let reason = resolution
            .reason(record)
            .ok_or_else(|| format!("{named} has no reason"))?;
        let explanation = resolution
            .explain(record)
            .ok_or_else(|| format!("{named} has no explanation"))?;
        let starts = match explanation.steps().first() {
            Some(Step::Installed {
                package,
                reason: first,
            }) => !kept && is(package, record) && first == reason,
            Some(Step::Kept {
                package,
                reason: first,
            }) => kept && is(package, record) && first == reason,
            _ => false,
        };
        if !starts {
            return Err(format!(
                "the explanation of {named} does not start with it:\n{explanation}"
            ));
        }
        if let Reason::Required(requirement) = reason {
            if !carries(record, requirement.name()) {
                return Err(format!(
                    "{named} is chosen for {requirement}, a name it does not provide"
                ));
            }
            if !requirement.answered_by(record) {
                return Err(format!(
                    "{named} is chosen for {requirement}, which it does not answer"
                ));
            }
        }
        check_steps(&explanation, true)
            .map_err(|problem| format!("{named}: {problem}\n{explanation}"))?;
    }
    for change in resolution.changes() {
        let gone = match *change {
            Change::Remove(record) => record,
            Change::Upgrade { from, .. }
            | Change::Downgrade { from, .. }
            | Change::Replace { from, .. } => from,
            Change::Install { .. } => continue,
        };
        let named = format!("{}-{}", gone.name(), gone.version());
        let exclusion = resolution
            .exclusion(gone)
            .ok_or_else(|| format!("{named} is dropped for no reason"))?;
        let explanation = resolution
            .explain(gone)
            .ok_or_else(|| format!("{named} is dropped with no explanation"))?;
        let starts = matches!(
            explanation.steps().first(),
            Some(Step::Dropped { package, exclusion: first })
                if is(package, gone) && first == exclusion
        );
        if !starts {
            return Err(format!(
                "the explanation of {named} does not start with it:\n{explanation}"
            ));
        }
        match exclusion {
            Exclusion::NotAdmitted(requirement) if requirement.answered_by(gone) => {
                return Err(format!(
                    "{named} is dropped for {requirement}, which it answers"
                ));
            }
            Exclusion::Forbidden(requirement) if !requirement.answered_by(gone) => {
                return Err(format!(
                    "{named} is dropped for {requirement}, which does not forbid it"
                ));
            }
            _ => {}
        }
        check_steps(&explanation, false)
            .map_err(|problem| format!("{named}: {problem}\n{explanation}"))?;
    }
    Ok(())
}

/// What a step of an explanation follows from, as it says.
#[derive(Debug)]
enum Cause<'e> {
    /// The step about this package.
    Package(&'e Package),
    /// The step saying what required this name.
    Name(&'e Name),
}

/// What `step` follows from, as far as it says: the package or name a
/// requirement came from, the packages a condition is met by, the package
/// holding a name. The packages a stranded dependency admits, and those
/// keeping a condition from holding, it does not name.
fn causes(step: &Step) -> Vec<Cause<'_>> {
    fn by(requirement: &Requirement) -> Vec<Cause<'_>> {
        match requirement.by() {
            RequiredBy::Package(package) => vec![Cause::Package(package)],
            RequiredBy::Candidates(name) => vec![Cause::Name(name)],
            RequiredBy::Request | RequiredBy::World => Vec::new(),
        }
    }
    match step {
        Step::Installed { reason, .. } | Step::Kept { reason, .. } => match reason {
            Reason::Required(requirement) => by(requirement),
            Reason::Condition(met_by) => met_by.iter().map(Cause::Package).collect(),
        },
        Step::Required(requirement) | Step::Sole { requirement, .. } => by(requirement),
        Step::RuledOut { exclusion, .. } | Step::Dropped { exclusion, .. } => match exclusion {
            Exclusion::NotAdmitted(requirement) | Exclusion::Forbidden(requirement) => {
                by(requirement)
            }
            Exclusion::Held { holder, .. } | Exclusion::Blocked { holder, .. } => {
                vec![Cause::Package(holder)]
            }
            Exclusion::Passed { completing, .. } => vec![Cause::Package(completing)],
            Exclusion::Stranded(_)
            | Exclusion::Uninstallable
            | Exclusion::Unrequired
            | Exclusion::Condition
            | Exclusion::Unmeetable(_)
            | Exclusion::Negated(_) => Vec::new(),
        },
    }
}

/// Whether `step` is what `cause` names.
fn is_cause(step: &Step, cause: &Cause<'_>) -> bool {
    match (step, cause) {
        (
            Step::Installed { package, .. }
            | Step::Kept { package, .. }
            | Step::RuledOut { package, .. }
            | Step::Dropped { package, .. }
            | Step::Sole { package, .. },
            Cause::Package(named),
        ) => package == *named,
        (Step::Required(requirement), Cause::Name(name)) => requirement.name() == *name,
        _ => false,
    }
}

/// Each step's causes, as it names them, come after it, so no chain of
/// steps returns to a step. With `chains`, the explanation of a package of
/// a resolution, whose `packages` these are: every step is about a package
/// installed or kept, a name required, or the only candidate left for a
/// name, whose dependencies are required before it is chosen; and one
/// following from nothing was required by the request or the world.
fn check_steps(explanation: &Explanation, chains: bool) -> Result<(), String> {
    let steps = explanation.steps();
    for (at, step) in steps.iter().enumerate() {
        let causes = causes(step);
        if chains {
            if !matches!(
                step,
                Step::Installed { .. } | Step::Kept { .. } | Step::Required(_) | Step::Sole { .. }
            ) {
                return Err(format!("a chain holds \"{step}\""));
            }
            if causes.is_empty() && !from_outside(step) {
                return Err(format!(
                    "\"{step}\" follows from nothing, and not from the request or the world"
                ));
            }
        }
        for cause in &causes {
            if steps[at + 1..].iter().any(|other| is_cause(other, cause)) {
                continue;
            }
            return Err(if steps[..=at].iter().any(|other| is_cause(other, cause)) {
                format!("\"{step}\" follows from {cause:?}, which comes before it")
            } else {
                format!("\"{step}\" follows from {cause:?}, which no step explains")
            });
        }
    }
    Ok(())
}

/// Whether `step` was required by the request or the world.
fn from_outside(step: &Step) -> bool {
    let requirement = match step {
        Step::Installed {
            reason: Reason::Required(requirement),
            ..
        }
        | Step::Kept {
            reason: Reason::Required(requirement),
            ..
        }
        | Step::Required(requirement)
        | Step::Sole { requirement, .. } => requirement,
        Step::Installed {
            reason: Reason::Condition(_),
            ..
        }
        | Step::Kept {
            reason: Reason::Condition(_),
            ..
        }
        | Step::RuledOut { .. }
        | Step::Dropped { .. } => return false,
    };
    matches!(requirement.by(), RequiredBy::Request | RequiredBy::World)
}

/// Whether `package` is `record`'s name and version.
fn is(package: &Package, record: &Record) -> bool {
    package.name() == record.name() && package.version().as_bytes() == record.version().as_bytes()
}

/// The root the checks resolve over besides an empty one: the first few
/// records of the first repository installed, and the first of them the
/// world.
pub fn root(repositories: &[Vec<Record>]) -> (Vec<Record>, Vec<Dependency>) {
    let installed: Vec<Record> = repositories
        .first()
        .map(|records| records.iter().take(3).cloned().collect())
        .unwrap_or_default();
    let world = installed
        .iter()
        .take(1)
        .filter_map(|record| Dialect::V3_0.dependency(record.name()).ok())
        .collect();
    (installed, world)
}

/// The requests the checks ask of a set of repositories: each name a record
/// carries, alone, and `extra`, in turn, each that reads.
pub fn requests(repositories: &[Vec<Record>], extra: &[Request]) -> Vec<Request> {
    let names: BTreeSet<&Name> = repositories.iter().flatten().map(Record::name).collect();
    names
        .into_iter()
        .filter_map(|name| Dialect::V3_0.dependency(name).ok())
        .map(|entry| [entry].into_iter().collect())
        .chain(extra.iter().cloned())
        .collect()
}
