//! The order a set of packages is installed in.
//!
//! The module documentation of [`resolve`](super) states the rules; this
//! one says how they are carried out.
//!
//! - **Placing.** A depth-first walk from the request entries, in name
//!   order, places each package after the packages its dependency entries
//!   lead to, taken in the order stated. A package the walk reaches while
//!   walking it is passed over there, which breaks a cycle where it closes.
//! - **Where an entry leads.** An entry leads to a package of the set that
//!   is a candidate for its name at a version it admits; a `!` entry, to one
//!   carrying or providing its name at any version. Where several do, the
//!   walk follows the one given first.
//! - **Triggers.** Placing a package reaches, through each name it carries
//!   and provides in turn, each package of the repositories whose
//!   `install_if` condition that name answers an entry of, or names in a
//!   `!` entry that does not forbid it, installed or not, and from each,
//!   the names that package carries and provides. In the order apk's table
//!   holds those names ([`table`](super::table)), a package of the set
//!   answering each is walked next, before the walk goes on, where it has
//!   no condition or each entry of its condition but its `!` entries is
//!   answered by a package the walk has reached, placed or being walked,
//!   and no `!` entry is. Which package: where the package whose condition
//!   the placement answers is not in the set, the first answering the name
//!   that may be walked; where it is in the set and not reached, the first
//!   such that its condition does not name; where it is being walked or
//!   placed, none.
//! - **The rest.** Packages nothing reaches are walked after, in the order
//!   given, each after what its dependencies lead to.

use alloc::vec;
use alloc::vec::Vec;

use super::Request;
use super::universe::{NameId, PackageId, Universe};
use crate::Name;
use crate::dependency::Dependency;
use crate::record::Record;

/// The order of `records` as apk installs them for `request`: each record's
/// index among them, every index once. The packages whose conditions lead
/// the walk are those of `repositories`, or failing that, of the set.
pub(super) fn order<'a>(
    records: impl IntoIterator<Item = &'a Record>,
    request: &Request,
    repositories: Option<&Universe<'_>>,
) -> Vec<usize> {
    placements(records, request, repositories)
        .records
        .into_iter()
        .map(|(index, _, _)| index)
        .collect()
}

/// What [`placements`] answers.
pub(super) struct Placed {
    /// Each record's index, in the order [`order`] gives, with the place in
    /// that order of the last record placing it led to (the packages its
    /// placement triggered, and what the walk placed for them), and of the
    /// first record its walk placed (its dependencies, or itself).
    pub(super) records: Vec<(usize, usize, usize)>,
    /// Each `!` entry the walk follows, of the request or of a package it
    /// walks, in the order followed: the place of the first record placed
    /// after it, and the name it states.
    pub(super) conflicts: Vec<(usize, Name)>,
}

/// The order of `records` as [`order`] gives it, with what the walk met on
/// the way (see [`Placed`]).
pub(super) fn placements<'a>(
    records: impl IntoIterator<Item = &'a Record>,
    request: &Request,
    repositories: Option<&Universe<'_>>,
) -> Placed {
    let mut universe = Universe::default();
    let packages: Vec<PackageId> = records
        .into_iter()
        .map(|record| universe.insert(record))
        .collect();
    let mut walk = Walk {
        universe: &universe,
        repositories: repositories.unwrap_or(&universe),
        marks: vec![Mark::Unvisited; universe.package_count()],
        placed: Vec::with_capacity(universe.package_count()),
        ends: Vec::with_capacity(universe.package_count()),
        began: vec![0; universe.package_count()],
        starts: Vec::with_capacity(universe.package_count()),
        conflicts: Vec::new(),
    };
    for entry in request.entries() {
        if entry.is_conflict() {
            walk.conflicts
                .push((walk.placed.len(), entry.name().clone()));
        }
        if let Some(package) = walk.leads_to(entry) {
            walk.visit(package);
        }
    }
    for &package in &packages {
        walk.visit(package);
    }

    // Each package's records, in the order given: a record that is a
    // package given before is placed with it.
    let mut records_of: Vec<Vec<usize>> = vec![Vec::new(); universe.package_count()];
    for (index, package) in packages.iter().enumerate() {
        if let Some(records) = records_of.get_mut(package.index()) {
            records.push(index);
        }
    }
    // Each package's place among the records placed, and where the records
    // its placement led to end.
    let mut first = Vec::with_capacity(walk.placed.len());
    let mut placed = Vec::with_capacity(packages.len());
    for package in &walk.placed {
        first.push(placed.len());
        if let Some(records) = records_of.get_mut(package.index()) {
            placed.append(records);
        }
    }
    let mut placements = Vec::with_capacity(placed.len());
    for (at, &end) in walk.ends.iter().enumerate() {
        let start = first.get(at).copied().unwrap_or_default();
        let stop = first
            .get(at.saturating_add(1))
            .copied()
            .unwrap_or(placed.len());
        let last = first
            .get(end.saturating_add(1))
            .copied()
            .unwrap_or(placed.len())
            .saturating_sub(1);
        let began = walk
            .starts
            .get(at)
            .and_then(|&began| first.get(began))
            .copied()
            .unwrap_or(start)
            .min(start);
        for &index in placed.get(start..stop).unwrap_or_default() {
            placements.push((index, last.max(start), began));
        }
    }
    let conflicts = core::mem::take(&mut walk.conflicts)
        .into_iter()
        .map(|(at, name)| (first.get(at).copied().unwrap_or(placed.len()), name))
        .collect();
    Placed {
        records: placements,
        conflicts,
    }
}

/// Where a package stands in the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Unvisited,
    Walking,
    Placed,
}

/// What the walk is doing at one level.
#[derive(Debug)]
enum Frame {
    /// Following the dependency entries of `package`; `next` is the one to
    /// follow next.
    Depends { package: PackageId, next: usize },
    /// Walking the packages placing the one placed at `placed` completed,
    /// in order.
    Triggers {
        packages: Vec<PackageId>,
        next: usize,
        placed: usize,
    },
}

struct Walk<'u, 'a, 'r> {
    /// The set being ordered.
    universe: &'u Universe<'a>,
    /// Every package that may lead the walk through its condition.
    repositories: &'r Universe<'r>,
    marks: Vec<Mark>,
    placed: Vec<PackageId>,
    /// For each package placed, the place of the last package placing it
    /// led to.
    ends: Vec<usize>,
    /// For each package, how many packages were placed when its walk
    /// began.
    began: Vec<usize>,
    /// For each package placed, how many packages were placed when its
    /// walk began.
    starts: Vec<usize>,
    /// Each `!` entry followed, with how many packages were placed then,
    /// and the name it states.
    conflicts: Vec<(usize, Name)>,
}

impl Walk<'_, '_, '_> {
    fn mark(&self, package: PackageId) -> Mark {
        self.marks
            .get(package.index())
            .copied()
            .unwrap_or(Mark::Placed)
    }

    fn set_mark(&mut self, package: PackageId, mark: Mark) {
        if let Some(slot) = self.marks.get_mut(package.index()) {
            *slot = mark;
        }
    }

    /// The package of the set `entry` leads the walk to, if any.
    fn leads_to(&self, entry: &Dependency) -> Option<PackageId> {
        let name = self.universe.id(entry.name())?;
        leads(self.universe, name, entry).next()
    }

    /// Places `root` after what its dependency entries lead to, then the
    /// packages placing it triggers, and so on for each.
    fn visit(&mut self, root: PackageId) {
        let mut stack: Vec<Frame> = Vec::new();
        self.enter(&mut stack, root);
        while let Some(top) = stack.last_mut() {
            let next = match top {
                Frame::Depends { package, next } => {
                    let (package, at) = (*package, *next);
                    *next = at.saturating_add(1);
                    match self.universe.package(package).record.depends().get(at) {
                        Some(entry) => {
                            if entry.is_conflict() {
                                self.conflicts
                                    .push((self.placed.len(), entry.name().clone()));
                            }
                            self.leads_to(entry)
                        }
                        None => {
                            stack.pop();
                            self.set_mark(package, Mark::Placed);
                            let placed = self.placed.len();
                            self.placed.push(package);
                            self.ends.push(placed);
                            let began = self.began.get(package.index()).copied().unwrap_or(placed);
                            self.starts.push(began);
                            let triggered = self.triggered(package);
                            if !triggered.is_empty() {
                                stack.push(Frame::Triggers {
                                    packages: triggered,
                                    next: 0,
                                    placed,
                                });
                            }
                            None
                        }
                    }
                }
                Frame::Triggers {
                    packages,
                    next,
                    placed,
                } => {
                    let at = *next;
                    *next = at.saturating_add(1);
                    let package = packages.get(at).copied();
                    if package.is_none() {
                        let end = self.placed.len().saturating_sub(1);
                        if let Some(slot) = self.ends.get_mut(*placed) {
                            *slot = end;
                        }
                        stack.pop();
                    }
                    package
                }
            };
            if let Some(next) = next {
                self.enter(&mut stack, next);
            }
        }
    }

    /// Starts walking `package`, unless the walk has reached it already.
    fn enter(&mut self, stack: &mut Vec<Frame>, package: PackageId) {
        if self.mark(package) == Mark::Unvisited {
            self.set_mark(package, Mark::Walking);
            let placed = self.placed.len();
            if let Some(slot) = self.began.get_mut(package.index()) {
                *slot = placed;
            }
            stack.push(Frame::Depends { package, next: 0 });
        }
    }

    /// The packages placing `package` triggers, in the order they are walked.
    fn triggered(&self, package: PackageId) -> Vec<PackageId> {
        let (universe, repositories) = (self.universe, self.repositories);
        let mut triggered: Vec<PackageId> = Vec::new();
        for &(name, version) in &universe.package(package).provides {
            let text = universe.name(name).text;
            let Some(conditions) = repositories.id(text) else {
                continue;
            };
            // Each name a package whose condition the placement answers
            // carries or provides, with where it leads.
            let mut carried: Vec<(NameId, Lead<'_>)> = Vec::new();
            for &other in &repositories.name(conditions).triggers {
                let other = repositories.package(other);
                // An entry answers the placement when it admits the package
                // placed, and a `!` entry when it does not forbid it.
                let answered = other.condition.iter().any(|(_, entry)| {
                    entry.name() == text
                        && entry.is_conflict()
                            != universe.admits(Some(entry), name, package, version)
                });
                if answered {
                    // The package itself, where the set holds it.
                    let lead = match universe.package_of(other.record) {
                        None => Lead::Any,
                        Some(itself) if self.mark(itself) == Mark::Unvisited => Lead::Unnamed(
                            other
                                .condition
                                .iter()
                                .filter(|(_, entry)| !entry.is_conflict())
                                .map(|(_, entry)| entry.name().as_bytes())
                                .collect(),
                        ),
                        Some(_) => Lead::Nowhere,
                    };
                    carried.extend(
                        other
                            .provides
                            .iter()
                            .map(|&(carried, _)| (carried, lead.clone())),
                    );
                }
            }
            carried.sort_by_cached_key(|(carried, _)| {
                repositories.position(repositories.name(*carried).text)
            });
            carried.dedup_by_key(|(carried, _)| *carried);
            for (in_repositories, lead) in carried {
                let Some(carried) = universe.id(repositories.name(in_repositories).text) else {
                    continue;
                };
                // The name leads to the first package of the set answering it
                // that the walk may take, as `Lead` allows
                // (`install-order/a-package-not-installed-leads-through-a-name-it-provides`,
                // `install-order/a-package-whose-condition-may-hold-leads-to-a-provider-it-does-not-name`,
                // `install-order/a-package-being-walked-leads-nowhere-through-its-names`).
                let unnamed = |next: PackageId| match &lead {
                    Lead::Any => true,
                    Lead::Unnamed(named) => !universe
                        .package(next)
                        .provides
                        .iter()
                        .any(|&(own, _)| named.contains(&universe.name(own).text)),
                    Lead::Nowhere => false,
                };
                let next = universe
                    .name(carried)
                    .providers
                    .iter()
                    .map(|&(next, _)| next)
                    .find(|&next| {
                        unnamed(next) && self.mark(next) == Mark::Unvisited && self.reached(next)
                    });
                if let Some(next) = next
                    && !triggered.contains(&next)
                {
                    triggered.push(next);
                }
            }
        }
        triggered
    }

    /// Whether each entry of `package`'s condition but its `!` entries is
    /// answered by a package the walk has reached, and no `!` entry by one
    /// (`install-order/a-condition-a-package-reached-forbids-is-not-walked-there`).
    fn reached(&self, package: PackageId) -> bool {
        let condition = &self.universe.package(package).condition;
        condition
            .iter()
            .filter(|(_, entry)| !entry.is_conflict())
            .all(|&(name, entry)| {
                leads(self.universe, name, entry).any(|other| self.mark(other) != Mark::Unvisited)
            })
            && !condition
                .iter()
                .filter(|(_, entry)| entry.is_conflict())
                .any(|&(name, entry)| {
                    self.universe
                        .admitted(name, Some(entry))
                        .any(|(other, _)| other != package && self.mark(other) != Mark::Unvisited)
                })
    }
}

/// Where a name a package whose condition a placement answers carries or
/// provides leads the walk.
#[derive(Debug, Clone)]
enum Lead<'r> {
    /// The package is not in the set: to any package of the set answering
    /// the name.
    Any,
    /// The package is in the set and not reached: to one its condition does
    /// not name.
    Unnamed(Vec<&'r [u8]>),
    /// The package is being walked or placed: nowhere.
    Nowhere,
}

/// The packages `entry` on `name` leads to, in the order added: candidates
/// at a version it admits, or for a `!` entry, every package carrying or
/// providing the name.
fn leads<'u>(
    universe: &'u Universe<'_>,
    name: NameId,
    entry: &'u Dependency,
) -> impl Iterator<Item = PackageId> + 'u {
    universe
        .admitted(name, (!entry.is_conflict()).then_some(entry))
        .map(|(package, _)| package)
}
