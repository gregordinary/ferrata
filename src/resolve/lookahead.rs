//! Preliminary choices: what apk would choose for each name it has yet to
//! decide, which steers the choices it makes before it reaches them.
//!
//! Before anything is decided, the names and turns a second walk reaches
//! get a preliminary choice, in the order it reaches them. That walk goes
//! through every candidate's dependencies and the names its condition
//! states, and to every package whose condition names a name reached
//! (`decision-order/a-name-reached-through-a-condition-is-chosen-for-there`).
//!
//! - **A name** takes the package chosen already that provides it, where
//!   one does (`decision-order/a-name-a-choice-answers-takes-that-package`),
//!   though that steers no name some package carries that the request or
//!   the world names itself
//!   (`decision-order/a-requested-name-some-package-carries-keeps-its-best-candidate-against-an-earlier-choice`);
//!   otherwise its best candidate that holds no name another choice
//!   provides, provides no name another holds, and is forbidden by no other
//!   choice. A choice provides the names it provides at no version for the
//!   names that chose otherwise, which give their choice up
//!   (`decision-order/a-choice-a-carried-package-answers-reserves-nothing`).
//! - **A turn** is chosen where the walk reaches it, or later, once the
//!   choices complete its condition
//!   (`install-if/a-condition-that-will-hold-steers-an-earlier-choice`,
//!   `install-if/a-condition-two-names-complete-steers-an-earlier-choice`),
//!   if it fits beside them. A package chosen for a name does not complete
//!   its own condition (`install-if/a-package-does-not-complete-its-own-condition`),
//!   and one whose condition forbids a name it holds is never chosen
//!   (`install-if/a-condition-forbidding-the-package-s-own-name-never-holds`).
//! - **A choice overrides** the earlier choices it contradicts: one its `!`
//!   entry forbids falls to that name's next candidate; one that cannot be
//!   installed beside any candidate of a dependency of it gives way to the
//!   best of those candidates, where that answers the same name, or else to
//!   the name's next candidate that can
//!   (`decision-order/a-later-name-whose-best-candidate-needs-a-candidate-steers-an-earlier-one`,
//!   `decision-order/a-dependency-whose-every-candidate-clashes-steers-an-earlier-choice`).
//!   A dependency whose one candidate apk would not choose for it, a
//!   provider at no version without a priority, overrides nothing
//!   (`decision-order/a-dependency-only-a-provider-apk-would-not-choose-answers-steers-nothing`).
//!   A package chosen for its turn overrides no choice that completes its
//!   condition
//!   (`install-if/a-package-whose-condition-holds-does-not-steer-the-choice-completing-it`,
//!   `install-if/a-package-whose-condition-holds-does-not-steer-the-choice-completing-a-later-name`,
//!   `install-if/a-package-whose-condition-holds-does-not-steer-a-provider-completing-it`,
//!   `decision-order/a-dependency-whose-condition-its-requirer-completes-does-not-steer-it`).
//!   A dependency of a name's choice overrides an earlier name's choice it
//!   cannot stand beside by a name one of them holds only where the walk
//!   through the choices reaches the later name without the earlier one
//!   (`decision-order/a-choice-reached-only-through-the-package-it-favors-steers-nothing`,
//!   `decision-order/a-version-whose-dependency-only-it-reaches-steers-nothing`,
//!   `reduced/octave-keeps-an-installed-libheif`).
//!   A choice for a name some package carries that the request or the
//!   world names, which its entry admits, stands against a turn and a name
//!   packages only provide, though not against a carried name
//!   (`decision-order/a-requested-name-some-package-carries-keeps-its-choice-against-a-provided-name`,
//!   `install-if/a-requested-name-some-package-carries-keeps-its-choice-against-a-turn`,
//!   `decision-order/a-requested-name-some-package-carries-follows-a-carried-name`).
//!   A name the request does not name whose best candidate's `!` entry
//!   forbids the choice for a name the request or the world names, which
//!   that name's entry admits, does not override it: it takes its next
//!   candidate that fits and forbids no such choice, where it has one
//!   (`conflicts/a-later-name-gives-way-to-a-requested-name-its-best-candidate-forbids`,
//!   `conflicts/a-requested-package-s-dependency-gives-way-to-it`,
//!   `installed/a-world-name-gives-way-to-a-requested-name`); a name the
//!   request names does not give way
//!   (`conflicts/a-conflict-between-two-requested-names-steers-neither`).
//!   Such a name gives way too to an earlier choice that depends on it
//!   (`conflicts/a-dependency-gives-way-to-a-choice-needing-it`,
//!   `installed/an-installed-dependency-gives-way-to-a-choice-needing-it`),
//!   unless that choice is for a name packages only provide, which neither
//!   the request nor the world names, and no candidate of another package
//!   could stand beside the choices in its place
//!   (`conflicts/a-dependency-gives-way-to-one-of-two-providers-needing-it`,
//!   `conflicts/a-dependency-does-not-give-way-to-the-one-provider-of-a-name-needing-it`).
//!   A chosen turn's dependency with a single candidate is installed with
//!   it, so that candidate answers each earlier name it provides
//!   (`install-if/a-dependency-of-a-condition-that-will-hold-answers-an-earlier-name`),
//!   but one whose choice completes the turn's condition
//!   (`install-if/a-dependency-with-one-candidate-does-not-answer-the-name-completing-the-condition`),
//!   and none where that candidate cannot be installed beside the turn's
//!   package
//!   (`install-if/a-dependency-with-one-candidate-that-cannot-stand-beside-its-package-answers-nothing`).
//!
//! A choice steers its name's decision where it overrides, where it is a
//! package chosen for another name, and, for a name packages only provide,
//! where an earlier choice kept the name from its best candidate
//! (`install-if/a-condition-that-never-holds-reserves-a-provider`). A
//! decision takes its name's steering choice where that is still a
//! candidate. A package chosen for its turn has a place in the decision
//! order; one not chosen has none, though its condition could hold
//! (`install-if/a-condition-the-choices-do-not-complete-numbers-nothing`).
//!
//! Which names the request reaches through the choices depends on the
//! choices, and the choices on which names count, so the choices are made
//! again until the names reached stay the same. The first time, every name
//! and turn gets a choice; after that, only those reached the time before
//! do. A name nothing requires makes no choice that keeps a required name
//! from its best candidate
//! (`decision-order/a-name-nothing-requires-reserves-nothing`). A chosen
//! turn counts where the packages chosen for the names reached complete
//! its condition, and its dependencies are reached through it.
//!
//! The choices are made at most four times. Where the names reached still
//! change the fourth time, the choices alternate: a name whose choice
//! steers another so that the walk no longer reaches it makes that choice
//! only every other time. Then no choice steers a decision, and no package
//! with a condition has a place in the decision order
//! (`decision-order/choices-that-do-not-settle-steer-nothing`). apk follows
//! one of the alternating choices in some such cases, where ferrata does
//! not
//! (`decision-order/choices-that-do-not-settle-pass-over-a-candidate-needing-what-it-forbids`,
//! `decision-order/choices-that-do-not-settle-pass-over-a-candidate-its-dependency-forbids`,
//! `decision-order/choices-that-do-not-settle-pass-over-a-candidate-whose-dependency-cannot-be-met`).

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use alloc::vec::Vec;

use super::Request;
use super::universe::{NameId, PackageId, Universe};
use crate::dependency::Dependency;
use crate::version::Version;

/// One step of the preliminary choices: a name to choose for, or a
/// package's turn to be chosen for its `install_if` condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Step {
    Name(NameId),
    Turn(PackageId),
}

/// What the preliminary choices settle before anything is decided.
#[derive(Debug, Default)]
pub(super) struct Preliminary {
    /// The choice for each name that steers its decision.
    pub(super) steering: BTreeMap<NameId, PackageId>,
    /// The packages chosen for their turn: those whose condition the
    /// choices complete.
    pub(super) turns: BTreeSet<PackageId>,
}

/// The preliminary choices, from `steps`, the order the walk for the
/// choices reaches names and turns in, for `request`, the world with the
/// request's entries in place of its own; `asked` is the request alone.
/// Where the choices do not settle within [`PASSES`], there are none: no
/// choice steers, and no turn is chosen.
pub(super) fn preliminary(
    universe: &Universe<'_>,
    steps: &[Step],
    open: &dyn Fn(PackageId) -> bool,
    request: &Request,
    asked: &Request,
) -> Preliminary {
    // Which steps count depends on the choices, and the choices on which
    // count: the passes repeat until the steps counted stay the same. The
    // first pass counts every step; each later one makes choices only for
    // the steps the one before reached.
    let requested: BTreeMap<NameId, &Dependency> = request
        .entries()
        .filter(|entry| !entry.is_conflict())
        .filter_map(|entry| Some((universe.id(entry.name())?, entry)))
        .collect();
    let asked: BTreeSet<NameId> = asked
        .entries()
        .filter(|entry| !entry.is_conflict())
        .filter_map(|entry| universe.id(entry.name()))
        .collect();
    let mut counting: Option<BTreeSet<Step>> = None;
    for _ in 0..PASSES {
        let mut pass = Pass {
            requested: &requested,
            asked: &asked,
            choices: Choices::new(universe, open),
            counting: counting.as_ref(),
            steering: BTreeSet::new(),
            blocked: BTreeSet::new(),
            visited: BTreeSet::new(),
        };
        for &step in steps {
            pass.take(step);
        }
        let reached = pass.choices.reached(request);
        if counting.as_ref() == Some(&reached) {
            let Pass {
                choices, steering, ..
            } = pass;
            let mut preliminary = Preliminary::default();
            for (step, package) in choices.by_step {
                match step {
                    Step::Name(name) => {
                        if steering.contains(&name) && reached.contains(&step) {
                            preliminary.steering.insert(name, package);
                        }
                    }
                    Step::Turn(_) => {
                        preliminary.turns.insert(package);
                    }
                }
            }
            return preliminary;
        }
        counting = Some(reached);
    }
    Preliminary::default()
}

/// How many times the choices are made before, the names reached still
/// changing, none is kept. Choices that have not settled by then nearly
/// always repeat an earlier set of names reached, so more passes would not
/// settle them.
const PASSES: usize = 4;

/// One pass of the preliminary choices.
struct Pass<'u, 'a, 'o, 'c> {
    /// The names the request and the world name, each with its entry.
    requested: &'c BTreeMap<NameId, &'c Dependency>,
    /// The names the request itself names, the world's aside.
    asked: &'c BTreeSet<NameId>,
    choices: Choices<'u, 'a, 'o>,
    /// The steps the pass before reached; none on the first pass, which
    /// counts every step.
    counting: Option<&'c BTreeSet<Step>>,
    /// The names whose choice steers their decision.
    steering: BTreeSet<NameId>,
    /// The names packages only provide that an earlier choice kept from
    /// their best candidate.
    blocked: BTreeSet<NameId>,
    /// The turns the walk has reached.
    visited: BTreeSet<PackageId>,
}

impl Pass<'_, '_, '_, '_> {
    fn counts(&self, step: Step) -> bool {
        self.counting
            .is_none_or(|counting| counting.contains(&step))
    }

    fn take(&mut self, step: Step) {
        if !self.counts(step) {
            return;
        }
        match step {
            Step::Name(name) => self.choose(name),
            Step::Turn(package) => {
                self.visited.insert(package);
                self.turn(package);
            }
        }
    }

    /// Makes the choice for `name`.
    fn choose(&mut self, name: NameId) {
        let choices = &mut self.choices;
        let ranked = choices.ranked(name);
        if let Some(answering) = choices.answering(name) {
            choices.insert(Step::Name(name), answering);
            let carried_and_requested =
                choices.universe.name(name).real && self.requested.contains_key(&name);
            if ranked.first() != Some(&answering) && !carried_and_requested {
                self.steering.insert(name);
            }
            return;
        }
        let mut fitting = ranked
            .iter()
            .copied()
            .filter(|&candidate| choices.fits(candidate));
        let Some(mut pick) = fitting.next() else {
            return;
        };
        let mut gives_way = false;
        if !self.asked.contains(&name)
            && ranked.first() == Some(&pick)
            && choices.yields(self.requested, name, pick)
            && let Some(next) =
                fitting.find(|&candidate| !choices.yields(self.requested, name, candidate))
        {
            pick = next;
            gives_way = true;
        }
        choices.insert(Step::Name(name), pick);
        if gives_way {
            self.steering.insert(name);
        }
        if choices.release(Step::Name(name), pick, &self.steering) {
            choices.refit(&mut self.blocked, &mut self.steering);
        }
        if ranked.first() != Some(&pick) && !choices.universe.name(name).real {
            self.steering.insert(name);
            self.blocked.insert(name);
        }
        choices.override_earlier(Step::Name(name), pick, &mut self.steering, self.requested);
        self.recheck(pick);
    }

    /// Chooses `package` for its turn, if its condition holds over the
    /// choices and it fits beside them, and reports whether it did.
    fn turn(&mut self, package: PackageId) -> bool {
        let step = Step::Turn(package);
        let choices = &mut self.choices;
        if choices.by_step.contains_key(&step)
            || !(choices.open)(package)
            || !choices.holds(package)
            || !choices.fits(package)
        {
            return false;
        }
        choices.insert(step, package);
        choices.release(step, package, &self.steering);
        choices.override_earlier(step, package, &mut self.steering, self.requested);
        choices.answer_earlier(package, &mut self.steering);
        true
    }

    /// Chooses each turn reached already whose condition `pick`, just
    /// chosen, completes, and each that one completes in turn.
    fn recheck(&mut self, pick: PackageId) {
        let universe = self.choices.universe;
        let mut chosen = vec![pick];
        while let Some(package) = chosen.pop() {
            for &(provided, _) in &universe.package(package).provides {
                for &trigger in &universe.name(provided).triggers {
                    if self.visited.contains(&trigger)
                        && self.counts(Step::Turn(trigger))
                        && self.turn(trigger)
                    {
                        chosen.push(trigger);
                    }
                }
            }
        }
    }
}

/// The preliminary choices made so far, indexed by what they hold, provide
/// and forbid.
struct Choices<'u, 'a, 'o> {
    universe: &'u Universe<'a>,
    open: &'o dyn Fn(PackageId) -> bool,
    by_step: BTreeMap<Step, PackageId>,
    /// The steps each chosen package is the choice for.
    steps_of: BTreeMap<PackageId, BTreeSet<Step>>,
    /// For each name, the chosen packages holding it.
    held: BTreeMap<NameId, BTreeSet<PackageId>>,
    /// For each name, the chosen packages providing it in any form.
    provided: BTreeMap<NameId, BTreeSet<PackageId>>,
    /// For each name, the chosen packages with a `!` entry on it.
    forbidding: BTreeMap<NameId, BTreeSet<PackageId>>,
}

impl<'u, 'a, 'o> Choices<'u, 'a, 'o> {
    fn new(universe: &'u Universe<'a>, open: &'o dyn Fn(PackageId) -> bool) -> Self {
        Choices {
            universe,
            open,
            by_step: BTreeMap::new(),
            steps_of: BTreeMap::new(),
            held: BTreeMap::new(),
            provided: BTreeMap::new(),
            forbidding: BTreeMap::new(),
        }
    }

    /// The open candidates for `name` a decision could choose, best first.
    fn ranked(&self, name: NameId) -> Vec<PackageId> {
        let universe = self.universe;
        let candidates = universe
            .name(name)
            .providers
            .iter()
            .filter(|&&(package, _)| (self.open)(package))
            .copied()
            .collect();
        universe
            .ranked(candidates)
            .into_iter()
            .map(|(package, _)| package)
            .collect()
    }

    fn insert(&mut self, step: Step, package: PackageId) {
        self.remove(step);
        self.by_step.insert(step, package);
        let steps = self.steps_of.entry(package).or_default();
        steps.insert(step);
        if steps.len() == 1 {
            self.index(package, true);
        }
    }

    fn remove(&mut self, step: Step) {
        let Some(package) = self.by_step.remove(&step) else {
            return;
        };
        let emptied = self.steps_of.get_mut(&package).is_some_and(|steps| {
            steps.remove(&step);
            steps.is_empty()
        });
        if emptied {
            self.steps_of.remove(&package);
            self.index(package, false);
        }
    }

    /// Adds `package` to the indexes, or takes it out of them.
    fn index(&mut self, package: PackageId, add: bool) {
        let candidate = self.universe.package(package);
        let entries = candidate
            .provides
            .iter()
            .flat_map(|&(name, version)| {
                let held = version.is_some().then_some((name, Index::Held));
                [Some((name, Index::Provided)), held]
            })
            .flatten()
            .chain(
                candidate
                    .conflicts
                    .iter()
                    .map(|&(name, _)| (name, Index::Forbidding)),
            );
        for (name, index) in entries {
            let map = match index {
                Index::Held => &mut self.held,
                Index::Provided => &mut self.provided,
                Index::Forbidding => &mut self.forbidding,
            };
            if add {
                map.entry(name).or_default().insert(package);
            } else if let Some(packages) = map.get_mut(&name) {
                packages.remove(&package);
            }
        }
    }

    /// The chosen packages other than `package` that cannot be installed
    /// beside it: by a name one holds that the other provides
    /// ([`Universe::holds_against`], found through the indexes of what each
    /// choice holds and provides), or by a `!` entry of theirs, or where
    /// `both_ways`, of its own.
    fn against(&self, package: PackageId, both_ways: bool) -> BTreeSet<PackageId> {
        let universe = self.universe;
        let candidate = universe.package(package);
        let mut against = BTreeSet::new();
        let others =
            |map: &BTreeMap<NameId, BTreeSet<PackageId>>, name: NameId| -> Vec<PackageId> {
                map.get(&name)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|&other| other != package)
                    .collect()
            };
        for &(name, version) in &candidate.provides {
            if version.is_some() {
                against.extend(others(&self.provided, name));
            }
            against.extend(others(&self.held, name));
            for other in others(&self.forbidding, name) {
                if forbids(universe, other, package) {
                    against.insert(other);
                }
            }
        }
        if both_ways {
            for &(name, _) in &candidate.conflicts {
                for other in others(&self.provided, name) {
                    if forbids(universe, package, other) {
                        against.insert(other);
                    }
                }
            }
        }
        against
    }

    /// Whether `candidate` fits beside every choice made.
    fn fits(&self, candidate: PackageId) -> bool {
        self.against(candidate, false).is_empty()
    }

    /// The chosen package that answers `name` already, if one does: the
    /// best of those providing it.
    fn answering(&self, name: NameId) -> Option<PackageId> {
        let universe = self.universe;
        let providing = self.provided.get(&name)?.iter().map(|&package| {
            let version = universe.package(package).candidacy(name).flatten();
            (package, version)
        });
        universe.best(providing).map(|(package, _)| package)
    }

    /// Whether `package`'s condition holds over the choices: each entry
    /// admits a chosen package other than `package`, and no `!` entry
    /// does.
    fn holds(&self, package: PackageId) -> bool {
        let universe = self.universe;
        let candidate = universe.package(package);
        let negated_alone = candidate
            .condition
            .iter()
            .all(|(_, entry)| entry.is_conflict());
        let names_one_chosen = || {
            candidate.condition.iter().any(|(name, _)| {
                self.provided
                    .get(name)
                    .is_some_and(|chosen| chosen.iter().any(|&other| other != package))
            })
        };
        !candidate.condition_forbids_itself
            && (!negated_alone || names_one_chosen())
            && candidate.condition.iter().all(|&(name, entry)| {
                let admitted = self.provided.get(&name).is_some_and(|chosen| {
                    chosen.iter().any(|&other| {
                        other != package && universe.answers(other, name, Some(entry))
                    })
                });
                admitted != entry.is_conflict()
            })
    }

    /// Applies what `pick`, the choice for `step`, contradicts in the other
    /// choices, but for the choices that stand against it
    /// ([`stands`]); `requested` holds the request's entries.
    fn override_earlier(
        &mut self,
        step: Step,
        pick: PackageId,
        steering: &mut BTreeSet<NameId>,
        requested: &BTreeMap<NameId, &Dependency>,
    ) {
        let universe = self.universe;
        for &(forbidden, _) in &universe.package(pick).conflicts {
            let chosen: Vec<PackageId> = self
                .provided
                .get(&forbidden)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&other| forbids(universe, pick, other))
                .collect();
            for other in chosen {
                if completes(universe, other, step) {
                    continue;
                }
                for earlier in self.steps_except(other, step) {
                    if stands(universe, requested, earlier, other, step) {
                        continue;
                    }
                    self.remove(earlier);
                    let Step::Name(earlier) = earlier else {
                        continue;
                    };
                    let next = self.ranked(earlier).into_iter().find(|&candidate| {
                        !forbids(universe, pick, candidate) && self.fits(candidate)
                    });
                    if let Some(next) = next {
                        self.insert(Step::Name(earlier), next);
                    }
                    steering.insert(earlier);
                }
            }
        }
        for &(needed, entry) in &universe.package(pick).depends {
            let answering: Vec<(PackageId, Option<&Version>)> = universe
                .admitted(needed, Some(entry))
                .filter(|&(package, _)| (self.open)(package))
                .collect();
            // Of several candidates, the best one is installed in the end. A
            // single one apk would not choose for the name, a provider at no
            // version without a priority, steers nothing
            // (`decision-order/a-dependency-only-a-provider-apk-would-not-choose-answers-steers-nothing`).
            let best = match answering.as_slice() {
                [] => continue,
                [only] if universe.choosable(*only) => only.0,
                [_] => continue,
                _ => {
                    let Some(best) = self.ranked(needed).into_iter().find(|&candidate| {
                        answering.iter().any(|&(package, _)| package == candidate)
                    }) else {
                        continue;
                    };
                    best
                }
            };
            for other in self.against(best, true) {
                if other == pick
                    || answering
                        .iter()
                        .any(|&(candidate, _)| compatible(universe, candidate, other))
                {
                    continue;
                }
                if completes(universe, other, step) {
                    continue;
                }
                let held = universe.holds_against(best, other);
                for earlier in self.steps_except(other, step) {
                    if stands(universe, requested, earlier, other, step)
                        || (held && !self.reaches_around(requested, step, earlier))
                    {
                        continue;
                    }
                    self.remove(earlier);
                    let Step::Name(earlier) = earlier else {
                        continue;
                    };
                    let answers = universe
                        .package(best)
                        .provides
                        .iter()
                        .any(|&(provided, _)| provided == earlier);
                    let next = if answers {
                        Some(best)
                    } else {
                        self.ranked(earlier)
                            .into_iter()
                            .find(|&candidate| compatible(universe, best, candidate))
                    };
                    if let Some(next) = next {
                        self.insert(Step::Name(earlier), next);
                    }
                    steering.insert(earlier);
                }
            }
        }
    }

    /// Takes back the choice of each other name `package`, chosen for
    /// `step`, provides at no version, and reports whether it took one
    /// back: `package` answers such a name, so its choice reserves nothing.
    /// A steering choice is kept.
    fn release(&mut self, step: Step, package: PackageId, steering: &BTreeSet<NameId>) -> bool {
        let mut released = false;
        for &(provided, version) in &self.universe.package(package).provides {
            let other = Step::Name(provided);
            if version.is_some() || other == step || steering.contains(&provided) {
                continue;
            }
            if self
                .by_step
                .get(&other)
                .is_some_and(|&chosen| chosen != package)
            {
                self.remove(other);
                released = true;
            }
        }
        released
    }

    /// Gives each name of `blocked` its best candidate again where that
    /// fits now, and the name stops steering; or else its best candidate
    /// that fits.
    fn refit(&mut self, blocked: &mut BTreeSet<NameId>, steering: &mut BTreeSet<NameId>) {
        for name in blocked.clone() {
            let ranked = self.ranked(name);
            let Some(&best) = ranked.first() else {
                continue;
            };
            if self.by_step.get(&Step::Name(name)) == Some(&best) {
                continue;
            }
            self.remove(Step::Name(name));
            if self.fits(best) {
                self.insert(Step::Name(name), best);
                blocked.remove(&name);
                steering.remove(&name);
            } else if let Some(next) = ranked.into_iter().find(|&candidate| self.fits(candidate)) {
                self.insert(Step::Name(name), next);
            }
        }
    }

    /// Gives each earlier name a dependency of `turn` with a single
    /// candidate provides to that candidate: a package installed for its
    /// condition has such a dependency installed with it at once
    /// (`install-if/a-dependency-with-one-candidate-is-installed-at-once`).
    /// A name whose choice completes `turn`'s condition keeps it
    /// ([`completes`]), and a candidate that cannot be installed beside
    /// `turn` answers nothing
    /// (`install-if/a-dependency-with-one-candidate-that-cannot-stand-beside-its-package-answers-nothing`).
    fn answer_earlier(&mut self, turn: PackageId, steering: &mut BTreeSet<NameId>) {
        let universe = self.universe;
        for &(needed, entry) in &universe.package(turn).depends {
            let mut answering = universe
                .admitted(needed, Some(entry))
                .filter(|&(package, _)| (self.open)(package))
                .map(|(package, _)| package);
            let (Some(only), None) = (answering.next(), answering.next()) else {
                continue;
            };
            if !compatible(universe, turn, only) {
                continue;
            }
            for &(provided, _) in &universe.package(only).provides {
                let step = Step::Name(provided);
                if provided != needed
                    && self.by_step.get(&step).is_some_and(|&chosen| {
                        chosen != only && !completes(universe, chosen, Step::Turn(turn))
                    })
                {
                    self.insert(step, only);
                    steering.insert(provided);
                }
            }
        }
    }

    /// Whether `candidate`, for `name`, would override through a `!` entry
    /// of its own the choice for a name the request or the world names,
    /// which that name's entry admits, or a choice that depends on `name`
    /// itself, but where that is a name's sole provider
    /// ([`Choices::provides_alone`]). A name the request does not name
    /// gives way to such a choice, taking its next candidate that fits and
    /// would override none
    /// (`conflicts/a-later-name-gives-way-to-a-requested-name-its-best-candidate-forbids`,
    /// `conflicts/a-requested-package-s-dependency-gives-way-to-it`,
    /// `conflicts/a-name-packages-only-provide-gives-way-to-a-requested-name`,
    /// `installed/a-world-name-gives-way-to-a-requested-name`,
    /// `conflicts/a-dependency-gives-way-to-a-choice-needing-it`,
    /// `conflicts/a-dependency-gives-way-to-one-of-two-providers-needing-it`,
    /// `installed/an-installed-dependency-gives-way-to-a-choice-needing-it`);
    /// one the request names does not
    /// (`conflicts/a-conflict-between-two-requested-names-steers-neither`).
    fn yields(
        &self,
        requested: &BTreeMap<NameId, &Dependency>,
        name: NameId,
        candidate: PackageId,
    ) -> bool {
        let universe = self.universe;
        universe
            .package(candidate)
            .conflicts
            .iter()
            .any(|&(forbidden, _)| {
                self.provided
                    .get(&forbidden)
                    .into_iter()
                    .flatten()
                    .filter(|&&other| forbids(universe, candidate, other))
                    .any(|&other| {
                        let needs = universe
                            .package(other)
                            .depends
                            .iter()
                            .any(|&(needed, _)| needed == name);
                        self.steps_except(other, Step::Name(name))
                            .into_iter()
                            .any(|earlier| {
                                let Step::Name(earlier) = earlier else {
                                    return false;
                                };
                                requested.get(&earlier).is_some_and(|&entry| {
                                    universe.answers(other, earlier, Some(entry))
                                }) || (needs && !self.provides_alone(requested, earlier, other))
                            })
                    })
            })
    }

    /// Whether `chosen`, the choice for `name`, is all `name` has: `name` is
    /// one packages only provide, which neither the request nor the world
    /// names, and no candidate for it of another package could stand
    /// beside the choices in its place. A dependency does not give way to
    /// such a choice
    /// (`conflicts/a-dependency-does-not-give-way-to-the-one-provider-of-a-name-needing-it`).
    fn provides_alone(
        &self,
        requested: &BTreeMap<NameId, &Dependency>,
        name: NameId,
        chosen: PackageId,
    ) -> bool {
        let universe = self.universe;
        !universe.name(name).real
            && !requested.contains_key(&name)
            && !self.ranked(name).into_iter().any(|candidate| {
                universe.package(candidate).name != universe.package(chosen).name
                    && self
                        .against(candidate, false)
                        .iter()
                        .all(|&other| other == chosen)
            })
    }

    /// The steps `package` is the choice for, `step` left out.
    fn steps_except(&self, package: PackageId, step: Step) -> Vec<Step> {
        self.steps_of
            .get(&package)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&other| other != step)
            .collect()
    }

    /// The steps a walk from the request reaches through the choices: from
    /// a name with a choice, that package's dependencies; from one without,
    /// its best candidate's; and each chosen turn whose condition the
    /// packages reached complete, with its dependencies.
    fn reached(&self, request: &Request) -> BTreeSet<Step> {
        let universe = self.universe;
        let roots = request
            .entries()
            .filter(|entry| !entry.is_conflict())
            .filter_map(|entry| universe.id(entry.name()));
        self.reached_from(roots, None)
    }

    /// The package the walk through the choices takes for `name`: its
    /// choice, or else its best candidate. A name's choice is always a
    /// candidate for it.
    fn taken(&self, name: NameId) -> Option<PackageId> {
        self.by_step
            .get(&Step::Name(name))
            .copied()
            .or_else(|| self.ranked(name).first().copied())
    }

    /// Whether the walk from the request through the choices
    /// ([`Choices::reached`]) reaches the name `step` chooses for without
    /// entering `earlier`'s. A choice for a name overrides an earlier
    /// choice it cannot stand beside by a name one of them holds only where
    /// it does (`decision-order/a-choice-reached-only-through-the-package-it-favors-steers-nothing`,
    /// `reduced/octave-keeps-an-installed-libheif`). Any other step reaches
    /// around.
    fn reaches_around(
        &self,
        requested: &BTreeMap<NameId, &Dependency>,
        step: Step,
        earlier: Step,
    ) -> bool {
        let (Step::Name(name), Step::Name(avoid)) = (step, earlier) else {
            return true;
        };
        self.reached_back(requested, name, avoid)
            .unwrap_or_else(|| {
                self.reached_from(requested.keys().copied(), Some(avoid))
                    .contains(&step)
            })
    }

    /// What a search back from `name` settles of whether the walk from
    /// `requested`'s names through the choices, never entering `avoid`
    /// ([`Choices::reached_from`]), reaches `name`, without walking all the
    /// request reaches. `Some(true)` where names alone lead to it from the
    /// request: the walk follows every name it reaches before any
    /// package's condition. `Some(false)` where nothing leads to it, even
    /// taking the condition of every chosen turn to hold that a package
    /// leading on could complete. `None` where only such a turn could lead
    /// to it, which only the walk settles.
    fn reached_back(
        &self,
        requested: &BTreeMap<NameId, &Dependency>,
        name: NameId,
        avoid: NameId,
    ) -> Option<bool> {
        let universe = self.universe;
        let mut seen: BTreeSet<NameId> = BTreeSet::new();
        let mut looked: BTreeSet<PackageId> = BTreeSet::new();
        // A package leads on where the walk takes it for a name, or reaches
        // it as a chosen turn: the names it is taken for are searched back
        // from, and a turn's later, once names alone have led nowhere.
        let mut look = |package: PackageId, stack: &mut Vec<NameId>, turns: &mut Vec<PackageId>| {
            if looked.insert(package) {
                stack.extend(
                    universe
                        .package(package)
                        .provides
                        .iter()
                        .map(|&(provided, _)| provided)
                        .filter(|&provided| self.taken(provided) == Some(package)),
                );
                if self.by_step.contains_key(&Step::Turn(package)) {
                    turns.push(package);
                }
            }
        };
        let mut stack = vec![name];
        let mut turns: Vec<PackageId> = Vec::new();
        let mut alone = true;
        loop {
            while let Some(current) = stack.pop() {
                if current == avoid || !seen.insert(current) {
                    continue;
                }
                if requested.contains_key(&current) {
                    return alone.then_some(true);
                }
                for package in universe.name(current).dependent_packages() {
                    look(package, &mut stack, &mut turns);
                }
            }
            if turns.is_empty() {
                return Some(false);
            }
            // A chosen turn is reached once a package reached answers each
            // entry of its condition that is not a `!` entry: the search goes
            // on from every package that could.
            alone = false;
            for turn in core::mem::take(&mut turns) {
                let condition = &universe.package(turn).condition;
                if condition.iter().all(|(_, entry)| entry.is_conflict()) {
                    return None;
                }
                for &(named, entry) in condition.iter().filter(|(_, entry)| !entry.is_conflict()) {
                    for (provider, _) in universe.admitted(named, Some(entry)) {
                        look(provider, &mut stack, &mut turns);
                    }
                }
            }
        }
    }

    /// The steps a walk from `roots` reaches through the choices, as
    /// [`Choices::reached`], never entering `avoid`.
    fn reached_from(
        &self,
        roots: impl IntoIterator<Item = NameId>,
        avoid: Option<NameId>,
    ) -> BTreeSet<Step> {
        let universe = self.universe;
        let mut reached: BTreeSet<Step> = BTreeSet::new();
        let mut live: BTreeSet<PackageId> = BTreeSet::new();
        let mut stack: Vec<NameId> = roots.into_iter().collect();
        let depends = |package: PackageId| {
            universe
                .package(package)
                .depends
                .iter()
                .map(|&(needed, _)| needed)
        };
        loop {
            while let Some(name) = stack.pop() {
                if Some(name) == avoid || !reached.insert(Step::Name(name)) {
                    continue;
                }
                if let Some(chosen) = self.taken(name) {
                    live.insert(chosen);
                    stack.extend(depends(chosen));
                }
            }
            for (&step, &package) in &self.by_step {
                if !matches!(step, Step::Turn(_)) || reached.contains(&step) {
                    continue;
                }
                let completed = universe
                    .package(package)
                    .condition
                    .iter()
                    .all(|&(name, entry)| {
                        entry.is_conflict()
                            || live
                                .iter()
                                .any(|&other| universe.answers(other, name, Some(entry)))
                    });
                if completed {
                    reached.insert(step);
                    live.insert(package);
                    stack.extend(depends(package));
                }
            }
            if stack.is_empty() {
                return reached;
            }
        }
    }
}

/// Which index an entry of a chosen package goes in.
#[derive(Debug, Clone, Copy)]
enum Index {
    Held,
    Provided,
    Forbidding,
}

/// Whether the choice of `chosen` for `earlier` stands against an override
/// by the choice for `by`. A name some package carries that the request or
/// the world names keeps a package its entry admits against a turn and
/// against a name packages only provide
/// (`decision-order/a-requested-name-some-package-carries-keeps-its-choice-against-a-provided-name`,
/// `install-if/a-requested-name-some-package-carries-keeps-its-choice-against-a-turn`).
/// A carried name's choice still overrides it
/// (`decision-order/a-requested-name-some-package-carries-follows-a-carried-name`),
/// and so does any choice where the entry does not admit it
/// (`decision-order/a-requested-name-some-package-carries-is-steered-from-a-package-it-does-not-admit`).
fn stands(
    universe: &Universe<'_>,
    requested: &BTreeMap<NameId, &Dependency>,
    earlier: Step,
    chosen: PackageId,
    by: Step,
) -> bool {
    let Step::Name(name) = earlier else {
        return false;
    };
    let Some(&entry) = requested.get(&name) else {
        return false;
    };
    let by_carried = matches!(by, Step::Name(by) if universe.name(by).real);
    universe.name(name).real && !by_carried && universe.answers(chosen, name, Some(entry))
}

/// Whether `package` is a candidate for a name the condition of the turn
/// `step` states: a package chosen for its turn overrides no choice that
/// completes its condition
/// (`install-if/a-package-whose-condition-holds-does-not-steer-the-choice-completing-it`).
fn completes(universe: &Universe<'_>, package: PackageId, step: Step) -> bool {
    let Step::Turn(turn) = step else {
        return false;
    };
    universe
        .package(turn)
        .condition
        .iter()
        .any(|&(name, _)| universe.package(package).candidacy(name).is_some())
}

/// Whether a `!` entry of `by` forbids `package`.
fn forbids(universe: &Universe<'_>, by: PackageId, package: PackageId) -> bool {
    by != package
        && universe
            .package(by)
            .conflicts
            .iter()
            .any(|&(name, entry)| universe.answers(package, name, Some(entry)))
}

/// Whether `a` and `b` may both be installed: neither holds a name the
/// other provides, and neither forbids the other.
fn compatible(universe: &Universe<'_>, a: PackageId, b: PackageId) -> bool {
    a == b
        || (!universe.holds_against(a, b) && !forbids(universe, a, b) && !forbids(universe, b, a))
}
