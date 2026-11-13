//! One resolution: a request's requirements, and the decisions that meet
//! them, in apk's order.
//!
//! The module documentation of [`resolve`](super) states the rules; this
//! one says how they are carried out.
//!
//! - **Preliminary choices** come first. They are made in the order of a
//!   walk from the request that follows the names a candidate's condition
//!   states, whatever version an entry states, and every trigger (see the
//!   `lookahead` module).
//! - **Numbering.** Before anything is decided, a depth-first walk from the
//!   request numbers every name it reaches, through every candidate of each
//!   name, whether chosen later or not. Candidates are walked in order of
//!   priority, then of the version they are candidates at, then of the order
//!   records were added in, and a candidate the dependency being followed
//!   does not admit is passed over
//!   (`decision-order/the-walk-takes-candidates-by-priority-first`). From
//!   each candidate the walk follows its dependencies, then the packages
//!   whose condition names the candidate's own name; after a name's
//!   candidates it follows the packages whose condition names the name, the
//!   name's triggers, but only those the preliminary choices chose. The
//!   triggers of a name are followed once. Each trigger is given a turn, a
//!   number of its own, and the walk goes on through it as through a
//!   candidate. Names packages only provide, and turns, are decided in this
//!   order; names some package carries in the reverse order. A name the
//!   walk does not reach is numbered after all it reaches, when it is first
//!   required.
//! - **Requirements** arrive on a queue, as packages are installed, and are
//!   applied in turn. Applying one records it, and makes its name pending
//!   unless a package installed already answers it; a pending name's
//!   candidates then share their common dependencies with it, by name, or
//!   where a single candidate is left, with the constraint of its first
//!   entry on each name; so does a candidate apk would choose once it is
//!   the only one left for a pending name.
//! - **Ruling out.** Each package is open, installed, or ruled out, and a
//!   package once ruled out stays so. A requirement rules out each provider
//!   of its name it does not admit; a package installed rules out every
//!   other package providing a name it holds, every package depending on
//!   that name in a way it does not meet, and every package a `!` entry of
//!   its own forbids; a pending name with a single candidate left rules out
//!   what installing that candidate would; and a package ruled out rules
//!   out in turn each package with a dependency that only ruled-out
//!   packages could meet
//!   (`decision-order/a-candidate-needing-only-ruled-out-packages-is-passed-over`).
//!   A dependency no package could ever meet rules nothing out. The open
//!   providers of a name are its candidates.
//! - **Decisions.** The pending item decided next is the name some package
//!   carries with the highest number, and failing that the first name
//!   packages only provide, or turn, by number. Before a carried name, or
//!   a requested name with a single candidate, is decided, the pending
//!   names that would require it are: each carried name a candidate of
//!   which depends on it, and each provided name whose one candidate
//!   depends on it or on a name a candidate of it provides, unless another
//!   name could still require that one; each after the names that would
//!   require it in turn. A name packages only provide, none of whose
//!   candidates apk would choose, is deferred, and decided again once no
//!   other name is pending, before the turns waiting on a `!` entry.
//! - **Turns.** At its turn a package is installed if its condition holds:
//!   every package the condition names installed, at a version it admits,
//!   and none it names with `!`. A condition holding only later, when a
//!   package it names is installed, installs the package then. A `!` entry
//!   naming a package that could still be installed waits until nothing
//!   else is left to decide. A package tied to what completes its
//!   condition that could not be installed at once is ruled out instead.
//! - **Notes.** As each package is installed, the solver notes why: the
//!   name it was chosen for, and how many requirements the name had then,
//!   or its condition. As each is ruled out, it notes what ruled it out. A
//!   resolution's explanations, and a refusal's, are read from these notes
//!   by [`Notes`]; noting costs a push for each package installed or ruled
//!   out. A package chosen for a name was chosen for the first of the
//!   name's requirements then that an entry stated, or failing that, the
//!   first: a name every candidate for another depends on is required
//!   before that name is decided, and the package installed for it then
//!   states the entry that says why.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Reverse;

use super::explain::{Exclusion, Explanation, Graph, Reason, Step};
use super::lookahead::Step as Choice;
use super::universe::{NameId, PackageId, Universe};
use super::{Package, Refusal, RefusalKind, Request, RequiredBy};
use crate::dependency::Dependency;
use crate::record::Record;
use crate::version::Version;

/// Resolves `world`, a root's world with the request's entries in place of
/// its own, over `universe`; `request` is what was asked, whose entries the
/// preliminary choices take before the rest of the world.
pub(super) fn solve<'a>(
    universe: &Universe<'a>,
    world: &Request,
    request: &Request,
) -> Result<Solved<'a>, Refusal> {
    let mut solve = Solve::new(universe);
    solve.run(world, request)?;
    let mut notes = Notes::new(&solve);
    let steps = solve
        .chosen
        .iter()
        .map(|&package| notes.add(Subject::Package(package)))
        .collect();
    let dropped = universe
        .held_packages()
        .filter(|&package| solve.fate(package) != Fate::Installed)
        .filter_map(|package| {
            let at = notes.add(Subject::Package(package))?;
            Some((universe.package(package).record, at))
        })
        .collect();
    Ok(Solved {
        packages: solve
            .chosen
            .iter()
            .map(|&id| universe.package(id).record)
            .collect(),
        steps,
        dropped,
        graph: notes.graph,
    })
}

/// What [`solve`] answers.
#[derive(Debug)]
pub(super) struct Solved<'a> {
    /// The packages apk installs, in the order its solver installs them.
    pub(super) packages: Vec<&'a Record>,
    /// Each package's step in `graph`, in the order of `packages`.
    pub(super) steps: Vec<Option<usize>>,
    /// Each package the root held that the resolution drops, with its step
    /// in `graph`, but those apk cannot install, which are not candidates.
    pub(super) dropped: Vec<(&'a Record, usize)>,
    /// Why each package stands where it does.
    pub(super) graph: Graph,
}

/// One requirement on a name.
#[derive(Debug, Clone, Copy)]
struct Requirement<'r> {
    /// The entry stating it; none where several candidates for another
    /// name all depend on the name, which requires it without a
    /// constraint. Where the other name has a single candidate, that
    /// candidate's first entry on the name, stated by [`Requirer::Sole`].
    entry: Option<&'r Dependency>,
    by: Requirer,
}

/// The names the `!` entries of `package`'s condition name.
fn negated_names<'u>(
    universe: &'u Universe<'_>,
    package: PackageId,
) -> impl Iterator<Item = NameId> + 'u {
    universe
        .package(package)
        .condition
        .iter()
        .filter(|(_, entry)| entry.is_conflict())
        .map(|&(named, _)| named)
}

/// Whether `package` holds a name of `names`: its own, or one it provides
/// at a version.
fn holds_any(universe: &Universe<'_>, package: PackageId, names: &BTreeSet<NameId>) -> bool {
    !names.is_empty()
        && universe
            .package(package)
            .holds()
            .any(|held| names.contains(&held))
}

impl Requirement<'_> {
    /// Whether the request or the world stated the requirement.
    fn is_outside(&self) -> bool {
        matches!(self.by, Requirer::Request | Requirer::World)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Requirer {
    Request,
    World,
    Package(PackageId),
    /// Every candidate for the name, each of which depends on the name
    /// required.
    Candidates(NameId),
    /// The single candidate for the name, which depends on the name
    /// required, before it is installed.
    Sole(NameId, PackageId),
}

/// Why a package was installed, as the solver notes it.
#[derive(Debug, Clone, Copy)]
enum Cause {
    /// It was chosen for this name, which had this many requirements then.
    Chosen(NameId, usize),
    /// Its condition held.
    Condition,
}

/// What ruled a package out, as the solver notes it: the first thing that
/// did, since a package is ruled out once.
#[derive(Debug, Clone, Copy)]
enum Ruling<'r> {
    /// A requirement on the name did not admit it.
    NotAdmitted(NameId, Requirement<'r>),
    /// A `!` entry on the name admitted it.
    Forbidden(NameId, Requirement<'r>),
    /// The package given holds the name, or the package ruled out holds it
    /// and the package given provides it at no version.
    Held(NameId, PackageId),
    /// The package given holds the name of the dependency, which does not
    /// admit it.
    Blocked(&'r Dependency, PackageId),
    /// The dependency on the name admits only packages ruled out.
    Stranded(NameId, &'r Dependency),
    /// The package, installed for its condition, could not be beside the
    /// package given, which completes the condition, for the dependency or
    /// `!` entry on the name.
    Passed(NameId, &'r Dependency, PackageId),
    /// The dependency on the name admits no package installed or open, and
    /// a candidate for the same name depends on a package up the chain of
    /// requirements or installed for the request.
    Unmeetable(NameId, &'r Dependency),
    /// The package, installed for its condition, depends on the name, and
    /// the dependency admits only packages holding a name a `!` entry of
    /// the condition names.
    Negated(NameId, &'r Dependency),
}

/// Which of two groups a pending item is decided in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// A name some package carries as its own, decided first.
    Carried,
    /// A name packages only provide, or a turn.
    Provided,
}

/// What the decision order holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    /// A name to decide.
    Name(NameId),
    /// A package's turn to be installed for its `install_if` condition.
    Turn(PackageId),
}

#[derive(Debug, Default)]
struct NameState<'r> {
    requirements: Vec<Requirement<'r>>,
    /// Whether the request or the world states a requirement on the name.
    asked: bool,
    /// How many candidates the name had when the dependencies they share
    /// were last required (see [`Solve::share`]); none until then.
    shared_among: usize,
    /// Whether a package installed provides the name, at a version or at
    /// none.
    answered: bool,
    /// The name's place in the decision order.
    number: Option<usize>,
    /// Whether the name is required, and not yet decided.
    pending: bool,
}

/// Where a package stands in a resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// It may yet be installed.
    Open,
    /// It is installed.
    Installed,
    /// It cannot be installed.
    RuledOut,
}

/// Where a package with an `install_if` condition stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    /// The walk has not given it a turn. Its condition is checked whenever
    /// a package it names is installed.
    Unreached,
    /// Its turn is in the decision order.
    Queued(usize),
    /// Its turn came while a `!` entry of its condition named a package
    /// that could still be installed; it waits until nothing else is left.
    Waiting,
    /// Its turn has passed. Its condition is checked again whenever a
    /// package it names is installed.
    Passed,
}

/// Whether a condition holds now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Condition {
    Holds,
    Fails,
    /// A `!` entry names a package that could still be installed.
    Undecided,
}

/// What a pending item is keyed by in [`Solve::pending`], which decides
/// the order items are decided in: its class, then [`order_key`].
type Pending = (Class, usize, Item);

/// Where an item numbered `number` goes among the pending items of its
/// class: a carried name numbered later goes first
/// (`decision-order/carried-names-are-decided-the-last-numbered-first`), a
/// provided name or turn numbered earlier does.
fn order_key(class: Class, number: usize) -> usize {
    match class {
        Class::Carried => usize::MAX.saturating_sub(number),
        Class::Provided => number,
    }
}

/// What the numbering walk follows.
#[derive(Debug, Clone, Copy)]
enum Edge<'r> {
    /// A name, through the entry naming it; an entry of a condition is
    /// followed to every candidate, whatever it admits.
    Name(NameId, Option<&'r Dependency>),
    /// A package whose condition names a name reached.
    Turn(PackageId),
    /// The packages whose condition names a candidate's own name, followed
    /// once for each name.
    Own(NameId),
}

/// Which order a walk makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The decision order: through dependencies, and to the packages whose
    /// condition names a name reached, where the preliminary choices chose
    /// them.
    Decisions,
    /// The order of the preliminary choices: through conditions as well,
    /// and to every package whose condition names a name reached, in the
    /// order of their names.
    Choices,
}

/// One frame of a walk: the edges still to follow from one name.
type Frame<'r> = (Vec<Edge<'r>>, usize);

/// How far [`Solve::is_stranded`] has looked among the providers of the
/// name a dependency names. A package once ruled out stays so, and a
/// provider the dependency does not admit never comes to be admitted, so
/// the providers looked past never need looking at again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stranding {
    /// Not looked at yet.
    Unknown,
    /// The provider at this place is admitted and was not ruled out when
    /// last looked at; every provider before it is ruled out or not
    /// admitted.
    Held(usize),
    /// Every provider admitted is ruled out: the dependency is stranded
    /// where it admits any.
    Spent(bool),
}

/// What the packages installed so far answer of a package's `install_if`
/// condition. Packages installed stay so, so an entry answered once stays
/// answered: [`Solve::select`] notes each as it installs the package that
/// answers it, and [`Solve::condition`] finds a condition that cannot hold
/// yet without looking through the providers of the names it states.
#[derive(Debug, Clone, Default)]
struct Answering {
    /// The entries, `!` entries aside, that no package installed answers.
    unmet: usize,
    /// Whether each entry has been answered, by its place in the
    /// condition; empty until a package installed answers one.
    met: Vec<bool>,
    /// Whether a `!` entry admits a package installed.
    forbidden: bool,
}

#[derive(Debug)]
struct Solve<'u, 'a: 'r, 'r> {
    universe: &'u Universe<'a>,
    names: Vec<NameState<'r>>,
    fates: Vec<Fate>,
    turns: Vec<Turn>,
    /// Packages a pending name has as its only candidate, whose names
    /// are held for them.
    certain: Vec<bool>,
    /// Each package made certain, with the name it is the only candidate
    /// for.
    certain_for: Vec<(PackageId, NameId)>,
    /// Packages ruled out whose dependents are still to be checked.
    ruled: Vec<PackageId>,
    /// How far ruling out has reached among the candidates each dependency
    /// admits, by package, then by the dependency's place; empty for a
    /// package none of whose dependencies has been asked about.
    stranding: Vec<Vec<Stranding>>,
    /// Each package ruled out, with what ruled it out.
    rulings: Vec<(PackageId, Ruling<'r>)>,
    /// Names whose candidates may have dwindled to one.
    narrowed: Vec<NameId>,
    /// The packages installed, in the order they were chosen.
    chosen: Vec<PackageId>,
    /// Why each package of `chosen` was installed, in the same order.
    causes: Vec<Cause>,
    /// Why each package was installed, by package, for those installed.
    installed_for: Vec<Option<Cause>>,
    /// Whether a package installed for its condition has a `!` entry in it.
    negating: bool,
    pending: BTreeSet<Pending>,
    work: VecDeque<(NameId, Requirement<'r>)>,
    /// Packages whose condition may have come to hold outside their turn.
    triggered: VecDeque<PackageId>,
    /// What the packages installed answer of each package's condition, by
    /// package; empty for a package nothing installed bears on yet.
    answering: Vec<Answering>,
    /// The turns waiting until nothing else is left, by number.
    waiting: BTreeSet<(usize, PackageId)>,
    /// Pending names packages only provide, none of whose candidates apk
    /// would choose, by number: decided once no other name is pending.
    deferred: BTreeSet<(usize, NameId)>,
    /// Whether the name being decided was deferred already, and is
    /// decided now for good.
    last: bool,
    /// Each name whose candidates' common dependency has been required,
    /// with that dependency.
    shared: BTreeSet<(NameId, NameId)>,
    /// Each name whose single candidate's dependency has been required
    /// with its constraint, with that dependency.
    sole_shared: BTreeSet<(NameId, NameId)>,
    /// The number the next name or turn numbered gets.
    numbered: usize,
    /// The preliminary choice for each name that has one, which its
    /// decision takes while it is still a candidate.
    preferred: BTreeMap<NameId, PackageId>,
    /// The packages the preliminary choices chose for their turn: the only
    /// ones the decision walk gives a turn.
    chosen_turns: BTreeSet<PackageId>,
    /// Each name the walk for the preliminary choices reaches.
    reached: Vec<bool>,
    /// Each name whose triggers the decision walk has followed.
    followed: Vec<bool>,
    /// The packages a requirement stands above: each provider of a name
    /// required, each provider of a name such a package depends on, and so
    /// on down (see [`Solve::require_below`]). Only a name required is ever
    /// pending, so the climb in [`Solve::requirers`] finds nothing up from
    /// a package not here.
    below: Vec<bool>,
    /// Each name whose providers are in `below`.
    below_names: Vec<bool>,
}

impl<'u, 'a: 'r, 'r> Solve<'u, 'a, 'r> {
    fn new(universe: &'u Universe<'a>) -> Solve<'u, 'a, 'r> {
        let mut names = Vec::new();
        names.resize_with(universe.name_count(), NameState::default);
        Solve {
            universe,
            names,
            fates: vec![Fate::Open; universe.package_count()],
            turns: vec![Turn::Unreached; universe.package_count()],
            certain: vec![false; universe.package_count()],
            certain_for: Vec::new(),
            ruled: Vec::new(),
            stranding: vec![Vec::new(); universe.package_count()],
            rulings: Vec::new(),
            narrowed: Vec::new(),
            chosen: Vec::new(),
            causes: Vec::new(),
            installed_for: vec![None; universe.package_count()],
            negating: false,
            pending: BTreeSet::new(),
            work: VecDeque::new(),
            triggered: VecDeque::new(),
            answering: vec![Answering::default(); universe.package_count()],
            waiting: BTreeSet::new(),
            deferred: BTreeSet::new(),
            last: false,
            shared: BTreeSet::new(),
            sole_shared: BTreeSet::new(),
            numbered: 0,
            preferred: BTreeMap::new(),
            chosen_turns: BTreeSet::new(),
            reached: vec![false; universe.name_count()],
            followed: vec![false; universe.name_count()],
            below: vec![false; universe.package_count()],
            below_names: vec![false; universe.name_count()],
        }
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "`names` holds one state for each name of the universe a `NameId` comes from"
    )]
    fn state(&self, name: NameId) -> &NameState<'r> {
        &self.names[name.index()]
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "`names` holds one state for each name of the universe a `NameId` comes from"
    )]
    fn state_mut(&mut self, name: NameId) -> &mut NameState<'r> {
        &mut self.names[name.index()]
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "`fates` holds one fate for each package of the universe a `PackageId` comes from"
    )]
    fn fate(&self, package: PackageId) -> Fate {
        self.fates[package.index()]
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "`fates` holds one fate for each package of the universe a `PackageId` comes from"
    )]
    fn set_fate(&mut self, package: PackageId, fate: Fate) {
        self.fates[package.index()] = fate;
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "`turns` holds one turn for each package of the universe a `PackageId` comes from"
    )]
    fn turn(&self, package: PackageId) -> Turn {
        self.turns[package.index()]
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "`turns` holds one turn for each package of the universe a `PackageId` comes from"
    )]
    fn set_turn(&mut self, package: PackageId, turn: Turn) {
        self.turns[package.index()] = turn;
    }

    /// Marks `package` as certain, and reports whether it was not before.
    #[expect(
        clippy::indexing_slicing,
        reason = "`certain` holds one flag for each package of the universe a `PackageId` comes from"
    )]
    fn make_certain(&mut self, package: PackageId) -> bool {
        !core::mem::replace(&mut self.certain[package.index()], true)
    }

    /// Rules `package` out, for `ruling`, unless it is installed or ruled
    /// out already.
    fn rule_out(&mut self, package: PackageId, ruling: Ruling<'r>) {
        if self.fate(package) == Fate::Open {
            self.set_fate(package, Fate::RuledOut);
            self.ruled.push(package);
            self.rulings.push((package, ruling));
        }
    }

    /// Rules out each package with a dependency that only packages ruled
    /// out could meet, and what each pending name left with one candidate
    /// rules out, until none is left to check.
    fn propagate(&mut self) {
        let universe = self.universe;
        loop {
            if let Some(gone) = self.ruled.pop() {
                for &(name, _) in &universe.package(gone).provides {
                    self.narrowed.push(name);
                    // A dependent with several entries on the name is ruled
                    // out by the first that is stranded.
                    for &(dependent, at) in &universe.name(name).dependents {
                        if self.fate(dependent) != Fate::Open {
                            continue;
                        }
                        let Some(&(_, entry)) = universe.package(dependent).depends.get(at) else {
                            continue;
                        };
                        if self.is_stranded(dependent, at, name, entry) {
                            self.rule_out(dependent, Ruling::Stranded(name, entry));
                        }
                    }
                }
                continue;
            }
            let Some(name) = self.narrowed.pop() else {
                return;
            };
            // A pending name with one candidate left will have it: the
            // names it holds are held for it already
            // (`decision-order/a-sole-candidate-holds-its-names-before-it-is-chosen`).
            if self.state(name).pending
                && let [only] = self.candidates(name).as_slice()
                && self.make_certain(only.0)
            {
                self.certain_for.push((only.0, name));
                self.claim(only.0);
                if self.universe.choosable(*only) && !self.is_answered(name) {
                    self.share_sole(name, only.0);
                }
            }
        }
    }

    /// Whether `entry`, on `name`, the dependency at `at` of `dependent`,
    /// admits some candidate for `name`, and every one it admits is ruled
    /// out. Asked again and again for one dependency as packages are ruled
    /// out, it looks at each provider of the name once in all, and at one
    /// while the provider it last found stays (see [`Stranding`]).
    fn is_stranded(
        &mut self,
        dependent: PackageId,
        at: usize,
        name: NameId,
        entry: &Dependency,
    ) -> bool {
        let universe = self.universe;
        let providers = &universe.name(name).providers;
        let Some(row) = self.stranding.get_mut(dependent.index()) else {
            return false;
        };
        if row.is_empty() {
            row.resize(
                universe.package(dependent).depends.len(),
                Stranding::Unknown,
            );
        }
        let Some(&known) = row.get(at) else {
            return false;
        };
        let from = match known {
            Stranding::Spent(stranded) => return stranded,
            Stranding::Held(place) => match providers.get(place) {
                Some(&(package, _)) if self.fate(package) != Fate::RuledOut => return false,
                _ => place.saturating_add(1),
            },
            Stranding::Unknown => 0,
        };
        let left = providers
            .iter()
            .enumerate()
            .skip(from)
            .find(|&(_, &(package, version))| {
                self.fate(package) != Fate::RuledOut
                    && universe.admits(Some(entry), name, package, version)
            })
            .map(|(place, _)| place);
        let now = match left {
            Some(place) => Stranding::Held(place),
            None => Stranding::Spent(universe.offers(name, entry)),
        };
        if let Some(state) = self
            .stranding
            .get_mut(dependent.index())
            .and_then(|row| row.get_mut(at))
        {
            *state = now;
        }
        now == Stranding::Spent(true)
    }

    fn run(&mut self, world: &'r Request, request: &Request) -> Result<(), Refusal> {
        // What the world forbids is ruled out before anything else.
        let by = |entry: &Dependency| {
            if request.get(entry.name()).is_some() {
                Requirer::Request
            } else {
                Requirer::World
            }
        };
        for entry in world.entries().filter(|entry| entry.is_conflict()) {
            if let Some(name) = self.universe.id(entry.name()) {
                self.forbid(name, entry, by(entry))?;
            }
        }
        self.propagate();

        // The preliminary choices come first: they say which turns the
        // decision order gives a place to.
        let steps = self.choice_order(world, request);
        for &step in &steps {
            if let Choice::Name(name) = step
                && let Some(reached) = self.reached.get_mut(name.index())
            {
                *reached = true;
            }
        }
        let fates = &self.fates;
        let open = |package: PackageId| fates.get(package.index()) == Some(&Fate::Open);
        let preliminary =
            super::lookahead::preliminary(self.universe, &steps, &open, world, request);
        self.preferred = preliminary.steering;
        self.chosen_turns = preliminary.turns;
        for entry in world.entries().filter(|entry| !entry.is_conflict()) {
            if let Some(name) = self.universe.id(entry.name()) {
                self.walk(name, entry);
            }
        }

        let mut requested = Vec::new();
        for entry in world.entries().filter(|entry| !entry.is_conflict()) {
            let requirement = Requirement {
                entry: Some(entry),
                by: by(entry),
            };
            let Some(name) = self.universe.id(entry.name()) else {
                // No record mentions the name, and the request or the world
                // stated it: there is nothing further to explain.
                return Err(Refusal::new(
                    RefusalKind::Unmet,
                    super::Requirement::new(
                        entry.name().clone(),
                        Some(entry.clone()),
                        self.required_by(requirement.by),
                    ),
                    None,
                    Explanation::default(),
                ));
            };
            self.work.push_back((name, requirement));
            requested.push(name);
        }
        self.drain()?;

        // A requested name with a single candidate, counted before anything
        // is installed, is installed first, after the names that would
        // require it
        // (`request/a-requested-name-with-one-candidate-is-settled-first`).
        self.settle(&requested, true)?;

        loop {
            self.flush()?;
            if let Some(&(class, _, item)) = self.pending.first() {
                match item {
                    Item::Name(name) => {
                        if class == Class::Carried {
                            self.decide_requirers(name)?;
                        }
                        self.decide(name)?;
                    }
                    Item::Turn(package) => self.take_turn(package)?,
                }
                continue;
            }
            // A name deferred for want of a candidate apk would choose is
            // decided now, unless a package installed since answers it.
            if let Some((_, name)) = self.deferred.pop_first() {
                self.state_mut(name).pending = false;
                self.last = true;
                let decided = self.decide(name);
                self.last = false;
                decided?;
                continue;
            }
            // Nothing else is left to decide: a turn waiting on a `!` entry
            // is taken now, the first by number first.
            let Some((_, package)) = self.waiting.pop_first() else {
                return Ok(());
            };
            self.set_turn(package, Turn::Passed);
            if self.fate(package) == Fate::Open && self.condition(package, true) == Condition::Holds
            {
                self.install_triggered(package)?;
            }
        }
    }

    /// Decides each name of `names` that is pending with a single
    /// candidate; for the request, after the names that would require it
    /// (`request/a-requested-name-with-one-candidate-is-settled-after-its-requirers`).
    fn settle(&mut self, names: &[NameId], requested: bool) -> Result<(), Refusal> {
        let single: Vec<NameId> = names
            .iter()
            .copied()
            .filter(|&name| self.candidates(name).len() == 1)
            .collect();
        for name in single {
            if requested && self.state(name).pending {
                self.decide_requirers(name)?;
            }
            if self.state(name).pending {
                self.decide(name)?;
                self.drain()?;
            }
        }
        Ok(())
    }

    /// Applies the requirements waiting, and installs each package whose
    /// condition has come to hold, until neither is left.
    fn flush(&mut self) -> Result<(), Refusal> {
        loop {
            self.drain()?;
            let Some(package) = self.triggered.pop_front() else {
                return Ok(());
            };
            if self.fate(package) != Fate::Open || self.turn(package) == Turn::Waiting {
                continue;
            }
            match self.condition(package, false) {
                Condition::Holds => self.install_triggered(package)?,
                Condition::Fails => {}
                Condition::Undecided => self.wait(package, None),
            }
        }
    }

    /// Numbers `root`, and every name and turn a depth-first walk from it
    /// reaches, following `entry` to it.
    fn walk(&mut self, root: NameId, entry: &'r Dependency) {
        if self.state(root).number.is_some() {
            return;
        }
        let number = self.assign_number();
        self.state_mut(root).number = Some(number);
        self.follow(root);
        let mut stack: Vec<Frame<'r>> =
            vec![(self.name_edges(root, Some(entry), Mode::Decisions), 0)];
        while let Some((edges, at)) = stack.last_mut() {
            let Some(&edge) = edges.get(*at) else {
                stack.pop();
                continue;
            };
            *at = at.saturating_add(1);
            match edge {
                Edge::Name(next, via) => {
                    if self.state(next).number.is_none() {
                        let number = self.assign_number();
                        self.state_mut(next).number = Some(number);
                        self.follow(next);
                        stack.push((self.name_edges(next, via, Mode::Decisions), 0));
                    }
                }
                Edge::Own(own) => {
                    if self.follow(own) {
                        stack.push((self.trigger_edges(own, Mode::Decisions), 0));
                    }
                }
                Edge::Turn(package) => {
                    if self.turn(package) == Turn::Unreached {
                        let number = self.assign_number();
                        self.set_turn(package, Turn::Queued(number));
                        self.pending
                            .insert((Class::Provided, number, Item::Turn(package)));
                        let mut edges = Vec::new();
                        self.package_edges(package, &mut edges, Mode::Decisions);
                        stack.push((edges, 0));
                    }
                }
            }
        }
    }

    /// Marks the triggers of `name` followed by the decision walk, and
    /// reports whether they were not before.
    fn follow(&mut self, name: NameId) -> bool {
        self.followed
            .get_mut(name.index())
            .is_some_and(|followed| !core::mem::replace(followed, true))
    }

    /// The order the preliminary choices are made in (see [`Mode::Choices`]):
    /// each name and each turn where a depth-first walk first reaches it,
    /// from the request's entries, then from the rest of the world's
    /// (`installed/a-choice-for-the-request-steers-a-world-name`).
    fn choice_order(&self, world: &'r Request, request: &Request) -> Vec<Choice> {
        let universe = self.universe;
        let mut order = Vec::new();
        let mut named = vec![false; universe.name_count()];
        let mut followed = vec![false; universe.name_count()];
        let mut turned = vec![false; universe.package_count()];
        let mark = |marks: &mut Vec<bool>, index: usize| {
            marks
                .get_mut(index)
                .is_some_and(|mark| !core::mem::replace(mark, true))
        };
        let asked = |entry: &&Dependency| request.get(entry.name()).is_some();
        let entries = world
            .entries()
            .filter(asked)
            .chain(world.entries().filter(|entry| !asked(entry)));
        let mut stack: Vec<Frame<'r>> = entries
            .filter(|entry| !entry.is_conflict())
            .filter_map(|entry| {
                let root = universe.id(entry.name())?;
                Some((vec![Edge::Name(root, Some(entry))], 0))
            })
            .collect();
        // The first entry is walked first.
        stack.reverse();
        while let Some((edges, at)) = stack.last_mut() {
            let Some(&edge) = edges.get(*at) else {
                stack.pop();
                continue;
            };
            *at = at.saturating_add(1);
            match edge {
                Edge::Name(next, via) => {
                    if mark(&mut named, next.index()) {
                        mark(&mut followed, next.index());
                        order.push(Choice::Name(next));
                        stack.push((self.name_edges(next, via, Mode::Choices), 0));
                    }
                }
                Edge::Own(own) => {
                    if mark(&mut followed, own.index()) {
                        stack.push((self.trigger_edges(own, Mode::Choices), 0));
                    }
                }
                Edge::Turn(package) => {
                    if mark(&mut turned, package.index()) {
                        order.push(Choice::Turn(package));
                        let mut edges = Vec::new();
                        self.package_edges(package, &mut edges, Mode::Choices);
                        stack.push((edges, 0));
                    }
                }
            }
        }
        order
    }

    /// The edges the walk follows from `name`, reached through `via`: from
    /// each candidate `via` admits, in the order the walk takes them, then
    /// to the packages whose condition names `name`.
    fn name_edges(&self, name: NameId, via: Option<&'r Dependency>, mode: Mode) -> Vec<Edge<'r>> {
        let mut providers: Vec<_> = self.universe.admitted(name, via).collect();
        providers.sort_by_key(|&(package, version)| {
            (
                Reverse(self.universe.package(package).record.priority()),
                Reverse(version),
                package,
            )
        });
        let mut edges = Vec::new();
        for &(package, _) in &providers {
            self.package_edges(package, &mut edges, mode);
        }
        edges.extend(self.trigger_edges(name, mode));
        edges
    }

    /// The turns of the packages whose condition names `name`: for the
    /// decision order, each the preliminary choices chose, in the order
    /// added (`install-if/a-condition-the-choices-do-not-complete-numbers-nothing`,
    /// `install-if/a-condition-that-never-holds-numbers-nothing`); for the
    /// preliminary choices, every one, in the order of their names
    /// (`install-if/conditions-a-name-completes-are-taken-in-name-order`).
    fn trigger_edges(&self, name: NameId, mode: Mode) -> Vec<Edge<'r>> {
        let universe = self.universe;
        let triggers = universe.name(name).triggers.iter().copied();
        let mut triggers: Vec<PackageId> = match mode {
            Mode::Decisions => triggers
                .filter(|trigger| self.chosen_turns.contains(trigger))
                .collect(),
            Mode::Choices => triggers.collect(),
        };
        if mode == Mode::Choices {
            triggers.sort_by_key(|&trigger| (universe.package(trigger).record.name(), trigger));
        }
        triggers.into_iter().map(Edge::Turn).collect()
    }

    /// The edges the walk follows from `package`: its dependencies; for
    /// the preliminary choices, the names its condition states too; and
    /// the packages whose condition names its own name
    /// (`install-if/a-condition-naming-a-candidate-has-its-turn-there`).
    fn package_edges(&self, package: PackageId, edges: &mut Vec<Edge<'r>>, mode: Mode) {
        let universe = self.universe;
        let candidate = universe.package(package);
        edges.extend(
            candidate
                .depends
                .iter()
                .map(|&(needed, entry)| Edge::Name(needed, Some(entry as &'r Dependency))),
        );
        if mode == Mode::Choices {
            edges.extend(
                candidate
                    .condition
                    .iter()
                    .map(|&(named, _)| Edge::Name(named, None)),
            );
        }
        edges.push(Edge::Own(candidate.name));
    }

    fn assign_number(&mut self) -> usize {
        let number = self.numbered;
        self.numbered = number.saturating_add(1);
        number
    }

    /// Applies the requirements waiting on the queue, and those they bring.
    fn drain(&mut self) -> Result<(), Refusal> {
        while let Some((name, requirement)) = self.work.pop_front() {
            self.apply(name, requirement)?;
        }
        Ok(())
    }

    fn apply(&mut self, name: NameId, requirement: Requirement<'r>) -> Result<(), Refusal> {
        let state = self.state_mut(name);
        state.requirements.push(requirement);
        state.asked |= requirement.is_outside();
        self.require_below(name);
        if requirement
            .entry
            .is_some_and(|entry| !entry.is_unconstrained())
        {
            // A requirement binds every provider of its name, one providing
            // it at no version included
            // (`providers/a-requirement-binds-a-provider-at-no-version`).
            let universe = self.universe;
            for &(package, version) in &universe.name(name).providers {
                if self
                    .universe
                    .admits(requirement.entry, name, package, version)
                {
                    continue;
                }
                if self.fate(package) == Fate::Installed {
                    return Err(self.refusal(
                        RefusalKind::Conflict,
                        name,
                        Some(requirement),
                        Some(package),
                    ));
                }
                self.rule_out(package, Ruling::NotAdmitted(name, requirement));
            }
        }
        if self.is_answered(name) {
            self.propagate();
            return Ok(());
        }
        self.make_pending(name);
        self.narrowed.push(name);
        self.propagate();
        self.share(name);
        Ok(())
    }

    /// Whether an installed package answers every requirement on `name`:
    /// its holder, or a package providing it at no version. Each
    /// requirement was checked against the packages installed as it came,
    /// and each package installed against the requirements before it, as a
    /// candidate the requirements admit.
    fn is_answered(&self, name: NameId) -> bool {
        self.state(name).answered
    }

    fn class(&self, name: NameId) -> Class {
        if self.universe.name(name).real {
            Class::Carried
        } else {
            Class::Provided
        }
    }

    fn make_pending(&mut self, name: NameId) {
        if self.state(name).pending {
            return;
        }
        let number = match self.state(name).number {
            Some(number) => number,
            None => {
                let number = self.assign_number();
                self.state_mut(name).number = Some(number);
                number
            }
        };
        let class = self.class(name);
        self.state_mut(name).pending = true;
        self.pending
            .insert((class, order_key(class, number), Item::Name(name)));
    }

    fn remove_pending(&mut self, name: NameId) {
        let state = self.state_mut(name);
        if !state.pending {
            return;
        }
        state.pending = false;
        let number = state.number.unwrap_or_default();
        let class = self.class(name);
        self.pending
            .remove(&(class, order_key(class, number), Item::Name(name)));
    }

    /// Requires each name every candidate for `name` depends on
    /// (`decision-order/a-dependency-every-candidate-shares-is-taken-first`):
    /// by name alone where several candidates share it
    /// (`decision-order/a-shared-dependency-carries-no-constraint`), and
    /// with the constraint of the first entry naming it where a single
    /// candidate is left
    /// (`decision-order/a-sole-candidate-s-dependency-is-required-with-its-constraint`,
    /// `decision-order/a-sole-candidate-s-first-entry-on-a-name-is-the-one-required`).
    ///
    /// A name's candidates only dwindle, so where they are as many as when
    /// this last ran, they are the same, and what they share is required
    /// already: each requirement on a name costs its providers, and only a
    /// change of candidates costs their dependencies again.
    fn share(&mut self, name: NameId) {
        let universe = self.universe;
        let candidates = self.candidates(name);
        if core::mem::replace(&mut self.state_mut(name).shared_among, candidates.len())
            == candidates.len()
        {
            return;
        }
        let Some(((first, _), rest)) = candidates.split_first() else {
            return;
        };
        let sole = rest.is_empty();
        let mut common: Vec<(NameId, &'r Dependency)> = universe
            .package(*first)
            .depends
            .iter()
            .map(|&(needed, entry)| (needed, entry as &'r Dependency))
            .filter(|&(needed, _)| needed != name)
            .collect();
        for &(other, _) in rest {
            if common.is_empty() {
                break;
            }
            let theirs: BTreeSet<NameId> = universe
                .package(other)
                .depends
                .iter()
                .map(|&(needed, _)| needed)
                .collect();
            common.retain(|(needed, _)| theirs.contains(needed));
        }
        for (needed, entry) in common {
            if sole {
                self.sole_shared.insert((name, needed));
            }
            if self.shared.insert((name, needed)) {
                self.work.push_back((
                    needed,
                    Requirement {
                        entry: sole.then_some(entry),
                        by: if sole {
                            Requirer::Sole(name, *first)
                        } else {
                            Requirer::Candidates(name)
                        },
                    },
                ));
            }
        }
    }

    /// Requires each name `only`, left the single candidate for `name`,
    /// depends on, with the constraint of its first entry on it, where that
    /// was not required so already: a candidate apk would choose, for a
    /// name no package installed answers
    /// (`decision-order/a-candidate-left-alone-requires-its-dependencies-with-their-constraints`,
    /// `reduced/libgedit-tepl-dev-upgrades-an-installed-libgedit-gfls`).
    fn share_sole(&mut self, name: NameId, only: PackageId) {
        for &(needed, entry) in &self.universe.package(only).depends {
            // A later entry on a name finds it in `sole_shared` already.
            if needed == name {
                continue;
            }
            if self.sole_shared.insert((name, needed)) {
                self.shared.insert((name, needed));
                self.work.push_back((
                    needed,
                    Requirement {
                        entry: Some(entry),
                        by: Requirer::Sole(name, only),
                    },
                ));
            }
        }
    }

    /// Adds the providers of `name`, required now, to the packages a
    /// requirement stands above, and each provider of a name one of them
    /// depends on, and so on down. Each name's providers are added once, so
    /// a resolution costs at most the records' names and entries once.
    fn require_below(&mut self, name: NameId) {
        let universe = self.universe;
        let mut names = vec![name];
        while let Some(name) = names.pop() {
            let Some(added) = self.below_names.get_mut(name.index()) else {
                continue;
            };
            if core::mem::replace(added, true) {
                continue;
            }
            for &(package, _) in &universe.name(name).providers {
                let Some(below) = self.below.get_mut(package.index()) else {
                    continue;
                };
                if !core::mem::replace(below, true) {
                    names.extend(
                        universe
                            .package(package)
                            .depends
                            .iter()
                            .map(|&(needed, _)| needed),
                    );
                }
            }
        }
    }

    /// Decides the pending names that would require `root`, a carried name
    /// or a requested one with a single candidate, before `root` is decided
    /// (`decision-order/a-requirer-is-decided-before-what-it-requires-z-m`).
    /// A name packages only provide is decided there only once nothing else
    /// could still require it (see [`Solve::waits_for_requirers`]), and the
    /// names that would require it are decided before it in turn
    /// (`decision-order/a-requirer-s-requirers-are-decided-first`).
    fn decide_requirers(&mut self, root: NameId) -> Result<(), Refusal> {
        let mut visited = BTreeSet::from([root]);
        let mut stack = vec![(root, self.requirers(root, &visited), 0)];
        while let Some((name, requirers, at)) = stack.last_mut() {
            let name = *name;
            let Some(&requirer) = requirers.get(*at) else {
                stack.pop();
                if name != root && self.state(name).pending {
                    self.decide(name)?;
                    self.flush()?;
                }
                continue;
            };
            *at = at.saturating_add(1);
            if !self.state(requirer).pending
                || (!self.universe.name(requirer).real && self.waits_for_requirers(requirer))
            {
                continue;
            }
            if visited.insert(requirer) {
                let requirers = self.requirers(requirer, &visited);
                stack.push((requirer, requirers, 0));
            }
        }
        Ok(())
    }

    /// Whether the name `name`, which packages only provide, waits rather
    /// than being decided before what its candidate needs: some package
    /// depending on it is open, and a candidate for another name that is
    /// pending or that the walk for the preliminary choices reaches, unless
    /// that name is answered already, or is a pending carried name, which is
    /// decided first in any case
    /// (`decision-order/a-requirer-another-pending-name-could-require-waits`,
    /// `decision-order/a-requirer-a-package-the-walk-reaches-depends-on-waits`,
    /// `decision-order/a-requirer-a-pending-carried-name-could-require-is-decided-first`).
    fn waits_for_requirers(&self, name: NameId) -> bool {
        let universe = self.universe;
        universe.name(name).dependent_packages().any(|dependent| {
            self.fate(dependent) == Fate::Open
                && universe
                    .package(dependent)
                    .provides
                    .iter()
                    .any(|&(other, _)| {
                        let state = self.state(other);
                        other != name
                            && (state.pending
                                || self.reached.get(other.index()).copied().unwrap_or(false))
                            && !(state.pending && universe.name(other).real)
                            && !self.is_answered(other)
                    })
        })
    }

    /// The pending names, outside `visited`, that would require `name`, by
    /// number: each carried name with a candidate depending on it, and each
    /// provided name whose one candidate depends on it, or on a name a
    /// candidate for `name` provides, where that candidate may be chosen
    /// for it
    /// (`decision-order/a-sole-candidate-with-a-real-dependency-is-installed-first`,
    /// `decision-order/a-requirer-reaches-through-the-names-a-candidate-provides`,
    /// `decision-order/a-requirer-whose-candidate-cannot-be-chosen-waits`);
    /// and each that would install a candidate for `name` otherwise: a
    /// carried name a candidate carries, and the names that would require
    /// one, through packages nothing requires yet
    /// (`decision-order/a-name-a-candidate-carries-is-decided-first`).
    ///
    /// Nothing changes while it looks, so it asks each name for its single
    /// candidate once, and keeps the names it has found and gone through
    /// in sets.
    fn requirers(&self, name: NameId, visited: &BTreeSet<NameId>) -> Vec<NameId> {
        let universe = self.universe;
        let mut found: Vec<(usize, NameId)> = Vec::new();
        let mut listed: BTreeSet<NameId> = BTreeSet::new();
        let mut single: BTreeMap<NameId, Option<PackageId>> = BTreeMap::new();
        let mut consider =
            |provided: NameId, dependent: Option<PackageId>, found: &mut Vec<(usize, NameId)>| {
                let state = self.state(provided);
                if !state.pending
                    || provided == name
                    || visited.contains(&provided)
                    || listed.contains(&provided)
                {
                    return;
                }
                let requires = universe.name(provided).real
                    || dependent.is_some_and(|dependent| {
                        let only = *single.entry(provided).or_insert_with(|| {
                            match self.candidates(provided).as_slice() {
                                [candidate] if universe.choosable(*candidate) => Some(candidate.0),
                                _ => None,
                            }
                        });
                        only == Some(dependent)
                    });
                if requires {
                    listed.insert(provided);
                    found.push((state.number.unwrap_or(usize::MAX), provided));
                }
            };
        let mut held: Vec<NameId> = vec![name];
        let mut holding: BTreeSet<NameId> = BTreeSet::from([name]);
        for (candidate, _) in self.candidates(name) {
            for &(provided, _) in &universe.package(candidate).provides {
                if holding.insert(provided) {
                    held.push(provided);
                }
            }
        }
        // Through the other names a candidate provides, only names
        // packages only provide require it: a carried name does not
        // (`decision-order/a-carried-name-through-a-provided-name-is-not-a-requirer`).
        for held in held {
            for dependent in universe.name(held).dependent_packages() {
                if self.fate(dependent) != Fate::Open {
                    continue;
                }
                for &(provided, _) in &universe.package(dependent).provides {
                    if held == name || !universe.name(provided).real {
                        consider(provided, Some(dependent), &mut found);
                    }
                }
            }
        }
        // From the name each candidate carries, up through what depends on
        // the names of packages nothing requires yet; not up from a package
        // no requirement stands above, where no name is pending.
        let mut seen: BTreeSet<PackageId> = BTreeSet::new();
        let mut climb: Vec<(PackageId, bool)> = self
            .candidates(name)
            .iter()
            .map(|&(package, _)| (package, true))
            .collect();
        while let Some((package, candidate)) = climb.pop() {
            if !seen.insert(package) {
                continue;
            }
            let provides = &universe.package(package).provides;
            // A candidate's own name comes first among the names it
            // provides; only that one leads up from a candidate.
            let leading = if candidate {
                provides.get(..1).unwrap_or_default()
            } else {
                provides.as_slice()
            };
            for &(provided, _) in leading {
                if provided == name {
                    continue;
                }
                if self.state(provided).pending {
                    consider(provided, None, &mut found);
                    continue;
                }
                for dependent in universe.name(provided).dependent_packages() {
                    if self.fate(dependent) != Fate::Open {
                        continue;
                    }
                    if universe
                        .package(dependent)
                        .provides
                        .iter()
                        .any(|&(other, _)| self.state(other).pending)
                    {
                        for &(other, _) in &universe.package(dependent).provides {
                            consider(other, Some(dependent), &mut found);
                        }
                    } else if self.below.get(dependent.index()).copied().unwrap_or(true) {
                        climb.push((dependent, false));
                    }
                }
            }
        }
        found.sort_unstable();
        found.into_iter().map(|(_, name)| name).collect()
    }

    /// Decides `name`: installs its best candidate, unless an installed
    /// package answers it, or defers a name packages only provide that
    /// has no candidate apk would choose.
    fn decide(&mut self, name: NameId) -> Result<(), Refusal> {
        self.remove_pending(name);
        if self.is_answered(name) {
            return Ok(());
        }
        self.pass_over_unmeetable(name);
        // A package holding a name a condition's `!` entry names is no
        // candidate below the package installed for that condition
        // (`install-if/a-condition-s-negated-entry-keeps-its-name-out-of-the-package-s-dependencies`,
        // `install-if/a-condition-s-negated-entry-leaves-another-candidate`,
        // `install-if/a-condition-s-negated-entry-refuses-a-dependency-further-down`).
        let negated = self.negated_above(name);
        let universe = self.universe;
        let mut candidates: Vec<_> = self
            .candidates(name)
            .into_iter()
            .filter(|&(package, _)| !holds_any(universe, package, &negated))
            .collect();
        if let Some(narrowed) = self.narrowing(name, &candidates) {
            candidates.retain(|(package, _)| narrowed.contains(package));
        }
        if let Some(&preferred) = self.preferred.get(&name)
            && candidates.iter().any(|&(package, _)| package == preferred)
        {
            return self.select(preferred, self.chosen_for(name));
        }
        let best = universe.best(
            candidates
                .iter()
                .copied()
                .filter(|&candidate| universe.choosable(candidate)),
        );
        match best {
            Some((package, _)) => self.select(package, self.chosen_for(name)),
            None if candidates.is_empty() => {
                Err(self.refusal(RefusalKind::Unmet, name, None, None))
            }
            // A name packages only provide waits, still pending, for
            // another to install a package answering it
            // (`decision-order/a-name-no-candidate-apk-would-choose-answers-is-decided-last`).
            None if !self.last && !self.universe.name(name).real => {
                let number = self.state(name).number.unwrap_or_default();
                self.state_mut(name).pending = true;
                self.deferred.insert((number, name));
                Ok(())
            }
            None => Err(self.refusal(RefusalKind::NoPriority, name, None, None)),
        }
    }

    /// The candidates of `candidates`, those for `name`, its decision is
    /// narrowed to: those of another name, pending and not answered, every
    /// candidate of which holds `name`, so that whichever answers it holds
    /// `name` too
    /// (`decision-order/a-name-every-candidate-of-which-holds-another-narrows-its-choice`,
    /// `decision-order/a-name-narrows-another-to-its-candidates-by-the-other-s-ranking`).
    /// A candidate of the other name that does not hold `name` narrows
    /// nothing
    /// (`decision-order/a-name-a-candidate-of-which-does-not-hold-another-narrows-nothing`),
    /// nor do two names narrowing to different candidates
    /// (`decision-order/two-names-narrowing-a-name-differently-narrow-nothing`),
    /// nor a name only packages installed for their condition require,
    /// which apk installs at their turn, after deciding `name`
    /// (`install-if/a-name-only-a-package-installed-for-its-condition-requires-narrows-nothing`).
    ///
    /// What another name narrows depends on that name alone, so each is
    /// asked once, however many candidates provide it; whether a package
    /// holds `name` is asked of the name's providers, gathered once.
    fn narrowing(
        &self,
        name: NameId,
        candidates: &[(PackageId, Option<&'a Version>)],
    ) -> Option<BTreeSet<PackageId>> {
        let universe = self.universe;
        let mut narrowed: Option<BTreeSet<PackageId>> = None;
        let mut asked: BTreeSet<NameId> = BTreeSet::new();
        let mut holders: Option<BTreeSet<PackageId>> = None;
        for &(candidate, _) in candidates {
            for &(other, _) in &universe.package(candidate).provides {
                if other == name
                    || !asked.insert(other)
                    || !self.state(other).pending
                    || self.is_answered(other)
                {
                    continue;
                }
                let by_condition = |requirement: &Requirement<'_>| {
                    matches!(requirement.by, Requirer::Package(package)
                        if matches!(self.cause_of(package), Some(Cause::Condition)))
                };
                if self.state(other).requirements.iter().all(by_condition) {
                    continue;
                }
                let theirs = self.candidates(other);
                let holders = holders.get_or_insert_with(|| {
                    universe
                        .name(name)
                        .providers
                        .iter()
                        .filter(|(_, version)| version.is_some())
                        .map(|&(package, _)| package)
                        .collect()
                });
                let holding = theirs.iter().all(|(package, _)| holders.contains(package));
                if theirs.is_empty() || !holding {
                    continue;
                }
                let set: BTreeSet<PackageId> = theirs.iter().map(|&(package, _)| package).collect();
                match &narrowed {
                    None => narrowed = Some(set),
                    Some(before) if *before == set => {}
                    Some(_) => return None,
                }
            }
        }
        narrowed.filter(|set| candidates.iter().any(|(package, _)| set.contains(package)))
    }

    /// Rules out each candidate for `name` with a dependency no package
    /// installed or open admits, where a candidate for it depends on a name
    /// a package other than itself answers: one up the chain of
    /// requirements that brought `name`, or one installed for the request
    /// or the world. apk passes those candidates over there, where
    /// elsewhere it would choose one and refuse
    /// (`refusals/a-candidate-depending-back-on-its-requirer-is-passed-over-when-unmet`,
    /// `refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-depending-on-its-requirer`,
    /// `refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-where-it-depends-on-its-requirer`,
    /// `refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-depending-further-up`).
    /// The chain runs through the name each package installed on it was
    /// chosen for, a name it only provides included
    /// (`refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-depending-further-up-through-a-provided-name`),
    /// but through the name a single candidate not yet installed carries
    /// (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over-beside-one-depending-further-up-through-single-providers`),
    /// and it ends at a package installed for its condition
    /// (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over-beside-one-depending-above-a-condition`).
    /// Not for a name the request or the world requires itself
    /// (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over-for-a-requested-name`),
    /// nor through a package beside the chain
    /// (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over-for-a-package-beside-the-chain`).
    fn pass_over_unmeetable(&mut self, name: NameId) {
        let universe = self.universe;
        if self.state(name).asked {
            return;
        }
        // Only a candidate with such a dependency is passed over; where none
        // has one, whether the name is tied to the chain changes nothing.
        let candidates = self.candidates(name);
        if candidates
            .iter()
            .all(|&(candidate, _)| self.unmeetable(candidate).is_none())
        {
            return;
        }
        // The packages a candidate depends on, itself aside: the name is
        // tied where one of them is installed for the request or the world,
        // or is up the chain of requirements that brought it.
        let wanted: BTreeSet<PackageId> = candidates
            .iter()
            .flat_map(|&(candidate, _)| {
                universe
                    .package(candidate)
                    .depends
                    .iter()
                    .flat_map(|&(needed, entry)| universe.admitted(needed, Some(entry)))
                    .map(|(other, _)| other)
                    .filter(move |&other| other != candidate)
            })
            .collect();
        let for_outside = |package: PackageId| {
            matches!(self.cause_of(package),
                Some(Cause::Chosen(chosen, _)) if self.state(chosen).asked)
        };
        if !wanted.iter().any(|&other| for_outside(other)) && !self.up_the_chain(name, &wanted) {
            return;
        }
        for (candidate, _) in candidates {
            if let Some((needed, entry)) = self.unmeetable(candidate) {
                self.rule_out(candidate, Ruling::Unmeetable(needed, entry));
            }
        }
        self.propagate();
    }

    /// Whether a package of `wanted` is up the chain of requirements that
    /// brought `name`: one that stated a requirement on it, or the only
    /// candidate of a name every candidate of which needs it, and so on
    /// up. The chain goes on from the name a package installed was chosen
    /// for, and from the name a single candidate not yet installed carries;
    /// a package installed for its condition, which no requirement brought,
    /// ends it.
    ///
    /// The walk visits each name once, and costs at most the requirements
    /// recorded so far; it stops at the first package of `wanted` it meets.
    /// [`Solve::negated_above`] walks up the same requirements, and goes on
    /// from a package installed in the same way; it says where the two
    /// differ.
    fn up_the_chain(&self, name: NameId, wanted: &BTreeSet<PackageId>) -> bool {
        let universe = self.universe;
        if wanted.is_empty() {
            return false;
        }
        let mut seen: BTreeSet<NameId> = BTreeSet::from([name]);
        let mut names = vec![name];
        while let Some(at) = names.pop() {
            for requirement in &self.state(at).requirements {
                let (package, next) = match requirement.by {
                    Requirer::Package(package) => match self.cause_of(package) {
                        Some(Cause::Chosen(chosen, _)) => (package, Some(chosen)),
                        _ => (package, None),
                    },
                    Requirer::Candidates(other) | Requirer::Sole(other, _) => {
                        match self.candidates(other).as_slice() {
                            [only] if universe.choosable(*only) => {
                                (only.0, Some(universe.package(only.0).name))
                            }
                            _ => continue,
                        }
                    }
                    Requirer::Request | Requirer::World => continue,
                };
                if wanted.contains(&package) {
                    return true;
                }
                if let Some(next) = next
                    && seen.insert(next)
                {
                    names.push(next);
                }
            }
        }
        false
    }

    /// The names a `!` entry names in the condition of a package installed
    /// for its condition that every requirement on `name` comes from,
    /// through the packages chosen for them; none where a requirement comes
    /// from the request or the world by another way.
    ///
    /// The walk goes up the requirements as [`Solve::up_the_chain`] does
    /// for [`Solve::pass_over_unmeetable`], through the name each package
    /// installed was chosen for, and stops at a package installed for its
    /// condition, whose `!` entries it collects. The two rules differ in
    /// two places: here a requirement
    /// from the request or the world ends the walk with no names, where the
    /// pass-over leaves that requirement aside and follows the others
    /// (`refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-depending-further-up`);
    /// and here a name every candidate of which needs the one below leads
    /// on to that name, where the pass-over takes its single candidate and
    /// goes on from the name the candidate carries
    /// (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over-beside-one-depending-further-up-through-single-providers`).
    fn negated_above(&self, name: NameId) -> BTreeSet<NameId> {
        let universe = self.universe;
        let mut negated = BTreeSet::new();
        if !self.negating {
            return negated;
        }
        let mut seen = BTreeSet::from([name]);
        let mut names = vec![name];
        while let Some(at) = names.pop() {
            for requirement in &self.state(at).requirements {
                let next = match requirement.by {
                    Requirer::Request | Requirer::World => return BTreeSet::new(),
                    Requirer::Candidates(other) | Requirer::Sole(other, _) => other,
                    Requirer::Package(package) => match self.cause_of(package) {
                        Some(Cause::Condition) => {
                            negated.extend(negated_names(universe, package));
                            continue;
                        }
                        Some(Cause::Chosen(chosen, _)) => chosen,
                        None => continue,
                    },
                };
                if seen.insert(next) {
                    names.push(next);
                }
            }
        }
        negated
    }

    /// Why `package` was installed, if it was.
    fn cause_of(&self, package: PackageId) -> Option<Cause> {
        self.installed_for.get(package.index()).copied().flatten()
    }

    /// The first dependency of `package` that no package installed or open
    /// admits.
    fn unmeetable(&self, package: PackageId) -> Option<(NameId, &'r Dependency)> {
        let universe = self.universe;
        universe
            .package(package)
            .depends
            .iter()
            .find(|&&(needed, entry)| {
                !universe
                    .admitted(needed, Some(entry))
                    .any(|(other, _)| self.fate(other) != Fate::RuledOut)
            })
            .map(|&(needed, entry)| (needed, entry as &'r Dependency))
    }

    /// The cause of a package chosen for `name` now.
    fn chosen_for(&self, name: NameId) -> Cause {
        Cause::Chosen(name, self.state(name).requirements.len())
    }

    /// Takes `package`'s turn: installs it if its condition holds.
    fn take_turn(&mut self, package: PackageId) -> Result<(), Refusal> {
        let Turn::Queued(number) = self.turn(package) else {
            return Ok(());
        };
        self.pending
            .remove(&(Class::Provided, number, Item::Turn(package)));
        if self.fate(package) != Fate::Open {
            self.set_turn(package, Turn::Passed);
            return Ok(());
        }
        match self.condition(package, false) {
            Condition::Holds => {
                self.set_turn(package, Turn::Passed);
                self.install_triggered(package)
            }
            Condition::Fails => {
                self.set_turn(package, Turn::Passed);
                Ok(())
            }
            Condition::Undecided => {
                self.wait(package, Some(number));
                Ok(())
            }
        }
    }

    /// Puts `package`'s turn among those waiting until nothing else is
    /// left, at `number`, or after every turn numbered so far.
    fn wait(&mut self, package: PackageId, number: Option<usize>) {
        let number = match (number, self.turn(package)) {
            (Some(number), _) => number,
            (None, Turn::Queued(number)) => {
                self.pending
                    .remove(&(Class::Provided, number, Item::Turn(package)));
                number
            }
            (None, _) => self.assign_number(),
        };
        self.set_turn(package, Turn::Waiting);
        self.waiting.insert((number, package));
    }

    /// Whether `package`'s condition holds: every entry is answered by an
    /// installed package at a version it admits, and no `!` entry is. With
    /// `last`, nothing is left to decide, and a package not installed never
    /// will be.
    ///
    /// A `!` entry on a name the package provides at no version admits the
    /// package itself, still open while its condition is asked, so the
    /// condition is undecided and waits until nothing else is left
    /// (`install-if/a-condition-forbidding-a-name-the-package-provides-at-no-version-waits-for-the-package-itself`,
    /// `install-if/a-condition-forbidding-a-name-the-package-provides-at-no-version-leaves-that-name-to-be-decided`).
    /// This differs from the preliminary choices' `Choices::holds` and the
    /// install order's `Walk::reached`, which leave the package itself out
    /// of what its `!` entries admit. Leaving it out here installs the
    /// package at once, where apk waits; on no generated case where the two
    /// differ does apk answer as leaving it out does.
    fn condition(&self, package: PackageId, last: bool) -> Condition {
        let universe = self.universe;
        let candidate = universe.package(package);
        if candidate.condition_forbids_itself {
            return Condition::Fails;
        }
        // What the packages installed answer: an entry none answers, or a
        // `!` entry one answers, fails the condition.
        let unmet = match self.answering.get(package.index()) {
            Some(answering) if !answering.met.is_empty() => {
                if answering.forbidden {
                    return Condition::Fails;
                }
                answering.unmet
            }
            _ => candidate
                .condition
                .iter()
                .filter(|(_, entry)| !entry.is_conflict())
                .count(),
        };
        if unmet > 0 {
            return Condition::Fails;
        }
        // A condition of `!` entries alone needs a package of a name it
        // names installed, at a version the entry does not forbid
        // (`install-if/a-condition-of-negated-entries-alone-needs-a-package-it-names`,
        // `install-if/a-condition-of-negated-entries-alone-needs-a-package-of-a-name-it-names-installed`).
        if !candidate
            .condition
            .iter()
            .any(|&(name, _)| self.is_answered(name))
        {
            return Condition::Fails;
        }
        // No package installed is one a `!` entry admits; one it admits
        // that is still open leaves the condition undecided.
        let undecided = !last
            && candidate
                .condition
                .iter()
                .filter(|(_, entry)| entry.is_conflict())
                .any(|&(name, entry)| {
                    universe
                        .admitted(name, Some(entry))
                        .any(|(other, _)| self.fate(other) == Fate::Open)
                });
        if undecided {
            Condition::Undecided
        } else {
            Condition::Holds
        }
    }

    /// Notes what `package`, installed now and a candidate for `name` at
    /// `version`, answers of the conditions naming `name`: each entry on
    /// `name` it admits, and each `!` entry on it that admits it.
    fn answer_conditions(&mut self, package: PackageId, name: NameId, version: Option<&Version>) {
        let universe = self.universe;
        for &trigger in &universe.name(name).triggers {
            let condition = &universe.package(trigger).condition;
            let Some(answering) = self.answering.get_mut(trigger.index()) else {
                continue;
            };
            if answering.met.is_empty() {
                answering.met = vec![false; condition.len()];
                answering.unmet = condition
                    .iter()
                    .filter(|(_, entry)| !entry.is_conflict())
                    .count();
            }
            for (&(named, entry), met) in condition.iter().zip(answering.met.iter_mut()) {
                if named != name || *met || !universe.admits(Some(entry), name, package, version) {
                    continue;
                }
                *met = true;
                if entry.is_conflict() {
                    answering.forbidden = true;
                } else {
                    answering.unmet = answering.unmet.saturating_sub(1);
                }
            }
        }
    }

    /// Installs `package` for its condition, or passes it over where it
    /// cannot be installed beside what completes the condition
    /// ([`Solve::passed_over`]). Each of its dependencies with a single
    /// candidate is installed at once
    /// (`install-if/a-dependency-with-one-candidate-is-installed-at-once`).
    fn install_triggered(&mut self, package: PackageId) -> Result<(), Refusal> {
        if let Some(ruling) = self.passed_over(package) {
            self.rule_out(package, ruling);
            self.propagate();
            return Ok(());
        }
        self.select(package, Cause::Condition)?;
        self.drain()?;
        let depends: Vec<NameId> = self
            .universe
            .package(package)
            .depends
            .iter()
            .map(|&(name, _)| name)
            .collect();
        self.settle(&depends, false)
    }

    /// What passes over `package`, whose condition holds, rather than
    /// refuse the request: a dependency of it that no package installed or
    /// open admits, or a `!` entry of it admitting a package installed,
    /// where it is tied to a package completing its condition, by depending
    /// on a name that package provides or by forbidding it
    /// (`install-if/an-unmet-dependency-passes-over-a-package-needing-its-condition`,
    /// `install-if/a-package-forbidding-what-completes-its-condition-is-passed-over`).
    /// A package tied to nothing its condition names refuses the request
    /// instead (`install-if/an-unmet-dependency-refuses-the-request`,
    /// `install-if/a-conflict-with-the-request-refuses-it`). Also a
    /// dependency of its own, not required already nor answered, that admits
    /// only packages holding a name a `!` entry of its condition names,
    /// whatever version the entry admits
    /// (`install-if/a-condition-s-negated-entry-keeps-its-name-out-of-the-package-s-dependencies`).
    fn passed_over(&self, package: PackageId) -> Option<Ruling<'r>> {
        let universe = self.universe;
        let candidate = universe.package(package);
        // The installed packages `entry` admits, `package` itself aside.
        let installed = |name: NameId, entry: &'r Dependency| {
            universe
                .admitted(name, Some(entry))
                .filter(move |&(other, _)| other != package && self.fate(other) == Fate::Installed)
                .map(|(other, _)| other)
        };
        let negated: BTreeSet<NameId> = negated_names(universe, package).collect();
        if !negated.is_empty() {
            for &(needed, entry) in &candidate.depends {
                // What the package's own dependencies leave alone: a name
                // required already, or answered by a package installed.
                if !self.state(needed).requirements.is_empty() || self.is_answered(needed) {
                    continue;
                }
                let mut admitted = universe
                    .admitted(needed, Some(entry))
                    .filter(|&(other, _)| self.fate(other) != Fate::RuledOut)
                    .peekable();
                if admitted.peek().is_some()
                    && admitted.all(|(other, _)| holds_any(universe, other, &negated))
                {
                    return Some(Ruling::Negated(needed, entry));
                }
            }
        }
        let completing = candidate
            .condition
            .iter()
            .filter(|(_, entry)| !entry.is_conflict())
            .flat_map(|&(name, entry)| installed(name, entry))
            .find(|&other| {
                let provider = universe.package(other);
                candidate
                    .depends
                    .iter()
                    .any(|&(needed, _)| provider.candidacy(needed).is_some())
                    || candidate
                        .conflicts
                        .iter()
                        .any(|&(name, entry)| installed(name, entry).any(|one| one == other))
            })?;
        let forbidding = candidate
            .conflicts
            .iter()
            .find(|&&(name, entry)| installed(name, entry).next().is_some())
            .copied();
        let (name, entry) = forbidding.or_else(|| self.unmeetable(package))?;
        Some(Ruling::Passed(name, entry, completing))
    }

    /// Installs `package`, one of the candidates, for `cause`: it holds
    /// each name it provides at a version, what it forbids is ruled out,
    /// and its dependencies are required.
    ///
    /// No package installed already clashes with it over a name: as each
    /// was installed, it ruled out every package it could not stand beside
    /// ([`Solve::claim`]), and a candidate is one not ruled out.
    fn select(&mut self, package: PackageId, cause: Cause) -> Result<(), Refusal> {
        let universe = self.universe;
        let installed = universe.package(package);
        if installed.conflicts_with_itself {
            return Err(self.conflicting_with_itself(package, cause));
        }
        self.set_fate(package, Fate::Installed);
        self.chosen.push(package);
        self.causes.push(cause);
        if let Some(slot) = self.installed_for.get_mut(package.index()) {
            *slot = Some(cause);
        }
        if matches!(cause, Cause::Condition) {
            self.negating |= negated_names(universe, package).next().is_some();
        }
        for &(name, version) in &installed.provides {
            self.state_mut(name).answered = true;
            self.answer_conditions(package, name, version);
        }
        self.claim(package);
        for &(name, entry) in &installed.conflicts {
            self.forbid(name, entry, Requirer::Package(package))?;
        }
        self.propagate();
        for &(name, _) in &installed.provides {
            self.triggered
                .extend(universe.name(name).triggers.iter().copied());
        }
        for &(name, entry) in &installed.depends {
            self.work.push_back((
                name,
                Requirement {
                    entry: Some(entry),
                    by: Requirer::Package(package),
                },
            ));
        }
        Ok(())
    }

    /// The refusal of `package`, chosen for `cause`, which conflicts with
    /// itself: of the name it was chosen for, or for a package whose
    /// condition holds, of its own name, by the first package installed
    /// that completes the condition.
    fn conflicting_with_itself(&self, package: PackageId, cause: Cause) -> Refusal {
        let universe = self.universe;
        match cause {
            Cause::Chosen(name, _) => {
                self.refusal(RefusalKind::ConflictsWithItself, name, None, Some(package))
            }
            Cause::Condition => {
                let candidate = universe.package(package);
                let completing = candidate
                    .condition
                    .iter()
                    .filter(|(_, entry)| !entry.is_conflict())
                    .find_map(|&(name, entry)| {
                        universe
                            .admitted(name, Some(entry))
                            .find(|&(other, _)| self.fate(other) == Fate::Installed)
                    });
                let requirement = completing.map(|(other, _)| Requirement {
                    entry: None,
                    by: Requirer::Package(other),
                });
                self.refusal(
                    RefusalKind::ConflictsWithItself,
                    candidate.name,
                    requirement,
                    Some(package),
                )
            }
        }
    }

    /// Rules out what `package` being installed excludes. A name it carries
    /// or provides at a version is held by it alone: every other package
    /// providing that name, at a version or at none, is ruled out, and so is
    /// every package depending on the name in a way `package`'s version does
    /// not meet. A name it provides at no version excludes every package
    /// holding the name (`providers/a-holder-excludes-a-provider-at-no-version`).
    /// The first two rules are [`Universe::holds_against`], applied name by
    /// name over the providers of each name `package` provides.
    fn claim(&mut self, package: PackageId) {
        let universe = self.universe;
        for &(name, version) in &universe.package(package).provides {
            for &(other, other_version) in &universe.name(name).providers {
                if other != package && (version.is_some() || other_version.is_some()) {
                    self.rule_out(other, Ruling::Held(name, package));
                }
            }
            if version.is_none() {
                continue;
            }
            // A dependent with several entries on the name is ruled out by
            // the first that does not admit the package.
            for &(dependent, at) in &universe.name(name).dependents {
                if dependent == package {
                    continue;
                }
                let Some(&(_, entry)) = universe.package(dependent).depends.get(at) else {
                    continue;
                };
                // A dependency no package could ever meet rules nothing
                // out, though a package installed holds its name
                // (`refusals/a-holder-does-not-rule-out-a-dependent-nothing-could-meet`).
                if !universe.admits(Some(entry), name, package, version)
                    && universe.offers(name, entry)
                {
                    self.rule_out(dependent, Ruling::Blocked(entry, package));
                }
            }
        }
    }

    /// Rules out each package `entry`, a `!` entry on `name` stated by `by`,
    /// forbids. A package's conflict does not reach the package itself
    /// (`conflicts/a-package-may-conflict-with-a-name-it-provides`).
    fn forbid(&mut self, name: NameId, entry: &'r Dependency, by: Requirer) -> Result<(), Refusal> {
        let universe = self.universe;
        let requirement = Requirement {
            entry: Some(entry),
            by,
        };
        for (package, _) in universe.admitted(name, Some(entry)) {
            if by == Requirer::Package(package) {
                continue;
            }
            if self.fate(package) == Fate::Installed {
                return Err(self.refusal(
                    RefusalKind::Forbidden,
                    name,
                    Some(requirement),
                    Some(package),
                ));
            }
            self.rule_out(package, Ruling::Forbidden(name, requirement));
        }
        Ok(())
    }

    /// The candidates for `name` apk may still choose: its open providers,
    /// which every requirement on every name they provide admits.
    fn candidates(&self, name: NameId) -> Vec<(PackageId, Option<&'a Version>)> {
        self.universe
            .name(name)
            .providers
            .iter()
            .filter(|&&(package, _)| self.fate(package) == Fate::Open)
            .copied()
            .collect()
    }

    /// The refusal of a requirement on `name`, for `kind`: of
    /// `requirement`, or where no one requirement is at fault, of the first
    /// one stated, with `holder` the package installed in its way.
    ///
    /// Its explanation starts from what stated the requirement, then the
    /// holder, then, where nothing can answer the name, each package ruled
    /// out that could have.
    fn refusal(
        &self,
        kind: RefusalKind,
        name: NameId,
        requirement: Option<Requirement<'r>>,
        holder: Option<PackageId>,
    ) -> Refusal {
        // An entry says more than a name every candidate for another
        // depends on.
        let requirements = &self.state(name).requirements;
        let requirement = requirement
            .or_else(|| {
                requirements
                    .iter()
                    .find(|requirement| requirement.entry.is_some())
                    .copied()
            })
            .or_else(|| requirements.first().copied());
        let mut roots: Vec<Subject> = Vec::new();
        roots.extend(requirement.and_then(|requirement| requirement.by.subject()));
        roots.extend(holder.map(Subject::Package));
        if kind == RefusalKind::Unmet {
            roots.extend(
                self.universe
                    .name(name)
                    .providers
                    .iter()
                    .filter(|&&(package, _)| self.fate(package) == Fate::RuledOut)
                    .map(|&(package, _)| Subject::Package(package)),
            );
        }
        let mut notes = Notes::new(self);
        let roots: Vec<usize> = roots
            .into_iter()
            .filter_map(|subject| notes.add(subject))
            .collect();
        Refusal::new(
            kind,
            match requirement {
                Some(requirement) => self.owned(name, requirement),
                // Only a name required is decided, and a refusal is of a
                // name decided or required, so this is not reached.
                None => super::Requirement::new(
                    self.universe.name(name).text.into(),
                    None,
                    RequiredBy::Request,
                ),
            },
            holder.map(|holder| Package::of(self.universe.package(holder).record)),
            notes.graph.explain(&roots),
        )
    }

    /// `requirement`, on `name`, as an explanation states it.
    fn owned(&self, name: NameId, requirement: Requirement<'r>) -> super::Requirement {
        super::Requirement::new(
            self.universe.name(name).text.into(),
            requirement.entry.cloned(),
            self.required_by(requirement.by),
        )
    }

    fn required_by(&self, by: Requirer) -> RequiredBy {
        match by {
            Requirer::Package(package) | Requirer::Sole(_, package) => {
                RequiredBy::Package(Package::of(self.universe.package(package).record))
            }
            Requirer::Candidates(other) => {
                RequiredBy::Candidates(self.universe.name(other).text.into())
            }
            Requirer::World => RequiredBy::World,
            Requirer::Request => RequiredBy::Request,
        }
    }
}

impl Requirer {
    /// What an explanation goes on to from a requirement stated by this:
    /// the package, the name every candidate for which depends on the name
    /// required, or the single candidate for a name, as it stood when it
    /// stated the requirement. The request and the world lead nowhere
    /// further.
    fn subject(self) -> Option<Subject> {
        match self {
            Requirer::Package(package) => Some(Subject::Package(package)),
            Requirer::Candidates(name) => Some(Subject::Name(name)),
            Requirer::Sole(name, package) => Some(Subject::Sole(package, name)),
            Requirer::Request | Requirer::World => None,
        }
    }
}

/// What one step of an explanation is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Subject {
    /// A package: installed, ruled out, or held by the root and dropped.
    Package(PackageId),
    /// A name required, and what first required it.
    Name(NameId),
    /// A package not installed, the only candidate left for a name.
    Sole(PackageId, NameId),
}

/// The solver's notes, read into the steps of explanations: each subject's
/// step is made once, and linked to the steps of what it follows from.
///
/// Each note points back in time. A package was installed for a requirement
/// applied before it was chosen, which a package installed before stated,
/// or the requirement of a name before it; a condition held through
/// packages installed before; a package was ruled out by something
/// installed or required before, or after the packages its dependency
/// needed were. So the steps form no cycle. They are made with an explicit
/// stack, since a chain of them can be as long as a closure is deep.
struct Notes<'s, 'u, 'a: 'r, 'r> {
    solve: &'s Solve<'u, 'a, 'r>,
    /// Each package installed: its place in the order installed, and why.
    causes: BTreeMap<PackageId, (usize, Cause)>,
    /// Each package ruled out, and what ruled it out.
    rulings: BTreeMap<PackageId, Ruling<'r>>,
    /// Each package made certain, and the name it was the only candidate
    /// for.
    certain: BTreeMap<PackageId, NameId>,
    /// The step made for each subject, or none where it has none.
    steps: BTreeMap<Subject, Option<usize>>,
    graph: Graph,
}

impl<'s, 'u, 'a: 'r, 'r> Notes<'s, 'u, 'a, 'r> {
    fn new(solve: &'s Solve<'u, 'a, 'r>) -> Notes<'s, 'u, 'a, 'r> {
        let mut certain = BTreeMap::new();
        for &(package, name) in &solve.certain_for {
            certain.entry(package).or_insert(name);
        }
        Notes {
            solve,
            causes: solve
                .chosen
                .iter()
                .zip(&solve.causes)
                .enumerate()
                .map(|(at, (&package, &cause))| (package, (at, cause)))
                .collect(),
            rulings: solve.rulings.iter().copied().collect(),
            certain,
            steps: BTreeMap::new(),
            graph: Graph::default(),
        }
    }

    /// The step for `subject`, made with the steps of everything it
    /// follows from, if it has one.
    fn add(&mut self, subject: Subject) -> Option<usize> {
        if let Some(&known) = self.steps.get(&subject) {
            return known;
        }
        let mut unlinked: Vec<(usize, Vec<Subject>)> = Vec::new();
        let at = self.make(subject, &mut unlinked);
        while let Some((from, causes)) = unlinked.pop() {
            for cause in causes {
                let to = match self.steps.get(&cause) {
                    Some(&known) => known,
                    None => self.make(cause, &mut unlinked),
                };
                if let Some(to) = to {
                    self.graph.link(from, to);
                }
            }
        }
        at
    }

    /// Makes the step for `subject`, leaving what it follows from in
    /// `unlinked`.
    fn make(
        &mut self,
        subject: Subject,
        unlinked: &mut Vec<(usize, Vec<Subject>)>,
    ) -> Option<usize> {
        let at = self.describe(subject).map(|(step, causes)| {
            let at = self.graph.push(step);
            unlinked.push((at, causes));
            at
        });
        self.steps.insert(subject, at);
        at
    }

    /// The step for `subject`, and the subjects it follows from.
    fn describe(&self, subject: Subject) -> Option<(Step, Vec<Subject>)> {
        let solve = self.solve;
        let universe = solve.universe;
        let package = match subject {
            Subject::Name(name) => {
                let requirement = *solve.state(name).requirements.first()?;
                return Some((
                    Step::Required(solve.owned(name, requirement)),
                    requirement.by.subject().into_iter().collect(),
                ));
            }
            Subject::Sole(package, name) => {
                let requirement = *solve.state(name).requirements.first()?;
                return Some((
                    Step::Sole {
                        package: Package::of(universe.package(package).record),
                        requirement: solve.owned(name, requirement),
                    },
                    requirement.by.subject().into_iter().collect(),
                ));
            }
            Subject::Package(package) => package,
        };
        let candidate = universe.package(package);
        let named = Package::of(candidate.record);
        // A package the root held and the resolution drops is named by the
        // root's record of it, as the change that drops it names it.
        let held = universe.held(package);
        let dropped = held.map(Package::of);
        match solve.fate(package) {
            Fate::Installed => {
                let &(_, cause) = self.causes.get(&package)?;
                let (reason, causes) = self.reason(package, cause)?;
                let step = if held.is_some() {
                    Step::Kept {
                        package: named,
                        reason,
                    }
                } else {
                    Step::Installed {
                        package: named,
                        reason,
                    }
                };
                Some((step, causes))
            }
            Fate::RuledOut => {
                let (exclusion, causes) = self.exclusion(*self.rulings.get(&package)?);
                let step = match dropped {
                    Some(package) => Step::Dropped { package, exclusion },
                    None => Step::RuledOut {
                        package: named,
                        exclusion,
                    },
                };
                Some((step, causes))
            }
            // A package the root held, neither installed nor ruled out:
            // nothing chose it. Any other open package has no step.
            Fate::Open => {
                let root = dropped?;
                let fails = !candidate.condition.is_empty()
                    && solve.condition(package, true) != Condition::Holds;
                let (exclusion, causes) = if fails {
                    (Exclusion::Condition, self.unmet_condition(package))
                } else {
                    (Exclusion::Unrequired, Vec::new())
                };
                Some((
                    Step::Dropped {
                        package: root,
                        exclusion,
                    },
                    causes,
                ))
            }
        }
    }

    /// Why `package`, installed for `cause`, is installed, and the subjects
    /// that follows from.
    fn reason(&self, package: PackageId, cause: Cause) -> Option<(Reason, Vec<Subject>)> {
        let solve = self.solve;
        let universe = solve.universe;
        match cause {
            Cause::Chosen(name, standing) => {
                let requirements = solve.state(name).requirements.get(..standing)?;
                let requirement = *requirements
                    .iter()
                    .find(|requirement| requirement.entry.is_some())
                    .or_else(|| requirements.first())?;
                // The single candidate that stated the requirement is
                // followed as the package it became, where it was installed
                // before this one.
                let installed_at = self.causes.get(&package).map(|&(at, _)| at);
                let subject = match requirement.by {
                    Requirer::Sole(_, sole)
                        if self
                            .causes
                            .get(&sole)
                            .is_some_and(|&(at, _)| Some(at) < installed_at) =>
                    {
                        Some(Subject::Package(sole))
                    }
                    by => by.subject(),
                };
                Some((
                    Reason::Required(solve.owned(name, requirement)),
                    subject.into_iter().collect(),
                ))
            }
            Cause::Condition => {
                // Each entry's answer is the package answering it that was
                // installed first: one installed before the condition held.
                // A `!` entry is answered by a package of the name it names
                // that it does not forbid, installed before this one, whose
                // installing had the condition checked; where none was, the
                // entry holds with nothing installed for it.
                let (installed_at, _) = *self.causes.get(&package)?;
                let mut met_by = Vec::new();
                let mut causes = Vec::new();
                for &(name, entry) in &universe.package(package).condition {
                    let first = universe
                        .name(name)
                        .providers
                        .iter()
                        .filter(|&&(other, version)| {
                            universe.admits(Some(entry), name, other, version)
                                != entry.is_conflict()
                        })
                        .filter_map(|&(other, _)| Some((self.causes.get(&other)?.0, other)))
                        .filter(|&(at, _)| at < installed_at)
                        .min();
                    if let Some((_, other)) = first {
                        met_by.push(Package::of(universe.package(other).record));
                        causes.push(Subject::Package(other));
                    }
                }
                Some((Reason::Condition(met_by), causes))
            }
        }
    }

    /// What keeps `package`'s condition from holding: for each entry no
    /// package installed answers, the packages it admits that were ruled
    /// out; for each `!` entry, the packages installed it admits. Packages
    /// dropped only because nothing chose them are not named, since they
    /// hold no place in time to follow from.
    fn unmet_condition(&self, package: PackageId) -> Vec<Subject> {
        let solve = self.solve;
        let universe = solve.universe;
        let mut causes = Vec::new();
        for &(name, entry) in &universe.package(package).condition {
            let admitted: Vec<PackageId> = universe
                .admitted(name, Some(entry))
                .map(|(other, _)| other)
                .collect();
            let installed = |other: &&PackageId| solve.fate(**other) == Fate::Installed;
            let named: Vec<PackageId> = if entry.is_conflict() {
                admitted.iter().filter(installed).copied().collect()
            } else if admitted.iter().any(|other| installed(&other)) {
                Vec::new()
            } else {
                admitted
                    .iter()
                    .filter(|&&other| solve.fate(other) == Fate::RuledOut)
                    .copied()
                    .collect()
            };
            causes.extend(named.into_iter().map(Subject::Package));
        }
        causes
    }

    /// The exclusion `ruling` states, and the subjects it follows from.
    fn exclusion(&self, ruling: Ruling<'r>) -> (Exclusion, Vec<Subject>) {
        let solve = self.solve;
        let universe = solve.universe;
        let by = |requirement: Requirement<'r>| requirement.by.subject().into_iter().collect();
        match ruling {
            Ruling::NotAdmitted(name, requirement) => (
                Exclusion::NotAdmitted(solve.owned(name, requirement)),
                by(requirement),
            ),
            Ruling::Forbidden(name, requirement) => (
                Exclusion::Forbidden(solve.owned(name, requirement)),
                by(requirement),
            ),
            Ruling::Held(name, holder) => (
                Exclusion::Held {
                    name: universe.name(name).text.into(),
                    holder: Package::of(universe.package(holder).record),
                },
                vec![self.holder(holder)],
            ),
            Ruling::Blocked(entry, holder) => (
                Exclusion::Blocked {
                    entry: entry.clone(),
                    holder: Package::of(universe.package(holder).record),
                },
                vec![self.holder(holder)],
            ),
            Ruling::Stranded(name, entry) => (
                Exclusion::Stranded(entry.clone()),
                universe
                    .admitted(name, Some(entry))
                    .map(|(package, _)| Subject::Package(package))
                    .collect(),
            ),
            Ruling::Negated(name, entry) => (
                Exclusion::Negated(entry.clone()),
                universe
                    .admitted(name, Some(entry))
                    .filter(|&(package, _)| solve.fate(package) != Fate::RuledOut)
                    .map(|(package, _)| Subject::Package(package))
                    .collect(),
            ),
            Ruling::Unmeetable(name, entry) => (
                Exclusion::Unmeetable(entry.clone()),
                universe
                    .admitted(name, Some(entry))
                    .filter(|&(package, _)| solve.fate(package) == Fate::RuledOut)
                    .map(|(package, _)| Subject::Package(package))
                    .collect(),
            ),
            // What completes the condition, then what the entry admits that
            // stands in the way: packages installed a `!` entry forbids, or
            // packages ruled out a dependency could have had.
            Ruling::Passed(name, entry, completing) => {
                let wanted = if entry.is_conflict() {
                    Fate::Installed
                } else {
                    Fate::RuledOut
                };
                let mut subjects = vec![Subject::Package(completing)];
                subjects.extend(
                    universe
                        .admitted(name, Some(entry))
                        .filter(|&(package, _)| {
                            package != completing && solve.fate(package) == wanted
                        })
                        .map(|(package, _)| Subject::Package(package)),
                );
                (
                    Exclusion::Passed {
                        entry: entry.clone(),
                        completing: Package::of(universe.package(completing).record),
                    },
                    subjects,
                )
            }
        }
    }

    /// What an explanation goes on to from `holder`, a package holding a
    /// name: the package installed, or the only candidate left for a name.
    fn holder(&self, holder: PackageId) -> Subject {
        match self.certain.get(&holder) {
            Some(&name) if self.solve.fate(holder) != Fate::Installed => {
                Subject::Sole(holder, name)
            }
            _ => Subject::Package(holder),
        }
    }
}
