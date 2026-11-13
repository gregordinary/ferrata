//! What a resolution changes in a root, in the order apk changes it.
//!
//! The module documentation of [`resolve`](super) states the rules; this
//! one says how they are carried out.
//!
//! - **Pairing.** A package of the resolution that is installed already
//!   (its identity) is no change. One that is not replaces the
//!   installed package of its name, if the resolution drops that one, and
//!   is installed otherwise. Each package installed that the resolution
//!   drops and nothing replaces is removed.
//! - **Waiting.** A removal waits for the changes to the installed packages
//!   that need it, through a dependency or an `install_if` condition: a
//!   package removed that needs it, and a package replaced that needs its
//!   own name, or a name it provides at a version that no package of the
//!   resolution provides. Packages that stay as they are hold nothing up.
//! - **Order.** First, the removals in the order the installed packages are
//!   given. Each goes at its place where it waits for nothing then, after
//!   the removals waiting on it through a name it provides at no version or
//!   through a condition, which it pulls: those through such a name first,
//!   each kind in the order of apk's table of names, each package at the
//!   first of the names it carries and provides, and each pulled in the
//!   same way. One needed through a name it provides at a version pulls
//!   nothing; one a package of the resolution displaces, providing its own
//!   name or a name it holds that a package kept or replaced needs, waits
//!   for that package. Any package displaced, sharing a name with a package
//!   of the resolution in any form, goes, at the latest, just before the
//!   walk placing that package, after the removals still waiting on it,
//!   whatever package replaced needed it. Then once more, but for those
//!   needed through a name provided at a version. Then the packages of the
//!   resolution in install order, each changed in its turn, after the
//!   removals its walk displaces; where the walk follows a `!` entry, of
//!   the request or of a package it walks, a removal still to come
//!   carrying or providing its name goes there, after the removals waiting
//!   on it. Whenever a package
//!   is removed or replaced, the removals waiting for it that carry a name
//!   it needs follow it: at once after a removal, and after a replacement
//!   once the walk has placed what placing it led to, the packages it
//!   triggered and what they needed. They follow in the reverse of the
//!   order a depth-first walk from it through those needs leaves them, a
//!   dependency's needs taken last stated first and a condition's in the
//!   order stated, each once nothing else waits on it. Of two entries
//!   naming one package, the last says how it is needed. The removals left
//!   come last, in the order given, each after the removals left waiting
//!   on it, which go first in the same way, in the order of the table of
//!   names; where removals wait on each other, the one reached last goes
//!   first; after each, the removals it frees that carry a name it needs,
//!   the one given last first.
//!

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

use super::Change;
use super::universe::{NameId, PackageId, Universe};
use crate::Name;
use crate::record::Record;

/// The changes that take a root holding `installed`, in the order given, to
/// one holding `packages`, in install order; `repositories` gives, for each
/// of `packages`, the repository it comes from, where the root does not hold
/// it already; `spans` gives, for each of
/// `packages`, the places among them of the first package its walk placed
/// and of the last package its placement led to; `conflicts` gives each
/// `!` entry the walk placing them followed, with the place of the first
/// package placed after it. `names` is the resolver's universe, which holds
/// the names in the order apk read them.
pub(super) fn changes<'a>(
    names: &Universe<'_>,
    installed: &[&'a Record],
    packages: &[&'a Record],
    repositories: &[Option<usize>],
    spans: &[(usize, usize)],
    conflicts: &[(usize, Name)],
) -> Vec<Change<'a>> {
    // The installed packages, and then the packages of the resolution that
    // change, in one universe: what each provides and needs. It holds every
    // record installed, those the resolver's universe passes over as apk
    // cannot install them included, since their needs order removals too.
    let mut universe = Universe::default();
    let old: Vec<PackageId> = installed
        .iter()
        .map(|&record| universe.insert(record))
        .collect();
    // Each installed package's index, by its place in the universe.
    let mut index_of = vec![None; universe.package_count()];
    for (at, &id) in old.iter().enumerate() {
        if let Some(slot) = index_of.get_mut(id.index())
            && slot.is_none()
        {
            *slot = Some(at);
        }
    }
    // What becomes of each installed package: kept, replaced by one of the
    // resolution's, or removed.
    let mut fates = vec![Fate::Removed; installed.len()];
    let mut steps: Vec<Step<'a>> = Vec::with_capacity(packages.len());
    // Each step's place among `packages`.
    let mut places: Vec<usize> = Vec::with_capacity(packages.len());
    for (place, &package) in packages.iter().enumerate() {
        let kept = universe
            .package_of(package)
            .and_then(|id| index_of.get(id.index()).copied().flatten());
        if let Some(at) = kept {
            if let Some(fate) = fates.get_mut(at) {
                *fate = Fate::Kept;
            }
            continue;
        }
        steps.push(Step::Install(package));
        places.push(place);
    }
    // The installed packages of each name the resolution does not keep, in
    // the order given: the first replaced by the first package of the name
    // that changes, and so on.
    let mut replaceable: BTreeMap<&Name, VecDeque<usize>> = BTreeMap::new();
    for (at, (&record, &fate)) in installed.iter().zip(&fates).enumerate() {
        if fate == Fate::Removed {
            replaceable.entry(record.name()).or_default().push_back(at);
        }
    }
    for step in &mut steps {
        let Step::Install(package) = *step else {
            continue;
        };
        let replaced = replaceable
            .get_mut(package.name())
            .and_then(VecDeque::pop_front);
        if let Some(at) = replaced {
            if let Some(fate) = fates.get_mut(at) {
                *fate = Fate::Replaced;
            }
            *step = Step::Replace(at, package);
        }
    }

    let new: Vec<PackageId> = steps
        .iter()
        .map(|step| match *step {
            Step::Install(record) | Step::Replace(_, record) => universe.insert(record),
        })
        .collect();

    // Where each removal a package of the resolution displaces waits for:
    // the start of the walk placing that package, and whether it waits
    // there though nothing else holds it up.
    let successors: Vec<Option<(usize, bool)>> = successors(&universe, &old, &new, &fates)
        .into_iter()
        .map(|found| {
            let (turn, held) = found?;
            let place = places.get(turn).copied()?;
            Some((spans.get(place).map_or(place, |&(start, _)| start), held))
        })
        .collect();
    let displaced = |at: usize| {
        successors
            .get(at)
            .copied()
            .flatten()
            .is_some_and(|(_, held)| held)
    };
    // The removals each `!` entry the walk follows names, carrying or
    // providing its name at whatever version, with the place it comes at.
    let reached: Vec<(usize, Vec<usize>)> = conflicts
        .iter()
        .map(|(place, name)| {
            let named = universe.id(name).into_iter().flat_map(|name| {
                universe
                    .admitted(name, None)
                    .filter_map(|(package, _)| index_of.get(package.index()).copied().flatten())
            });
            let named = named
                .filter(|&at| fates.get(at) == Some(&Fate::Removed))
                .collect();
            (*place, named)
        })
        .collect();
    let mut reached = reached.into_iter().peekable();
    let mut order = Order::new(
        &universe, &old, &index_of, installed, &fates, packages, names,
    );
    for at in 0..installed.len() {
        if !displaced(at) {
            order.pull(at, &displaced);
        }
    }
    // A second time, for the removals the first freed through a name
    // provided at no version, which come before the rest
    // (`installed/a-provider-at-no-version-goes-after-the-first-removals`).
    for at in 0..installed.len() {
        if order.ready(at) && !order.late(at) && !displaced(at) {
            order.remove(at);
        }
    }
    // The removals a package replaced frees, each with the place its
    // placement's walk ends at: they follow once the walk has placed what
    // placing it led to
    // (`installed/a-package-replaced-frees-what-it-needed-after-what-it-triggers`).
    let mut freed: Vec<(usize, usize)> = Vec::new();
    // The removals packages of the resolution displace, by the start of the
    // walk placing the package that displaces each: once that walk comes,
    // each is due.
    let mut coming: Vec<(usize, usize)> = successors
        .iter()
        .enumerate()
        .filter_map(|(at, found)| found.map(|(start, _)| (start, at)))
        .collect();
    coming.sort_unstable();
    let mut coming = coming.into_iter().peekable();
    let mut due: BTreeSet<usize> = BTreeSet::new();
    for (turn, step) in steps.into_iter().enumerate() {
        let place = places.get(turn).copied().unwrap_or_default();
        order.follow(&mut freed, |end| end < place);
        // A removal a `!` entry the walk follows names goes there, after the
        // removals waiting on it
        // (`installed/a-removal-a-conflict-entry-leads-the-walk-to-goes-there`,
        // `installed/a-removal-a-world-conflict-leads-the-walk-to-goes-there`).
        while let Some((_, named)) = reached.next_if(|&(at, _)| at <= place) {
            for at in named {
                order.reach(at);
            }
        }
        // A removal a package of the resolution displaces comes just before
        // the walk that places that package, in the order given, after the
        // removals still waiting on it
        // (`installed/a-displaced-package-goes-before-its-successor-after-the-removals-waiting-on-it`).
        while let Some((_, at)) = coming.next_if(|&(start, _)| start <= place) {
            due.insert(at);
        }
        if !due.is_empty() {
            let now: Vec<usize> = due.iter().copied().collect();
            for at in now {
                if order.displaced_ready(at) {
                    order.remove(at);
                } else {
                    order.reach(at);
                }
            }
            due.retain(|&at| order.waiting(at));
        }
        // A package that changes is one the root did not hold, so it comes
        // from a repository.
        let Some(repository) = repositories.get(place).copied().flatten() else {
            continue;
        };
        match step {
            Step::Install(package) => order.changes.push(Change::Install {
                package,
                repository,
            }),
            Step::Replace(at, package) => {
                let Some(&from) = installed.get(at) else {
                    continue;
                };
                let to = package;
                order
                    .changes
                    .push(match package.version().cmp(from.version()) {
                        Ordering::Greater => Change::Upgrade {
                            from,
                            to,
                            repository,
                        },
                        Ordering::Less => Change::Downgrade {
                            from,
                            to,
                            repository,
                        },
                        Ordering::Equal => Change::Replace {
                            from,
                            to,
                            repository,
                        },
                    });
                let end = spans.get(place).map_or(place, |&(_, end)| end);
                order.free(at);
                freed.push((end, at));
            }
        }
        order.follow(&mut freed, |end| end <= place);
    }
    for (_, named) in reached {
        for at in named {
            order.reach(at);
        }
    }
    order.follow(&mut freed, |_| true);
    order.last();
    order.changes
}

/// For each installed package to be removed, of the packages `old`, the
/// first of the packages `new`, the resolution's that change, that
/// displaces it, providing in any form a name it provides, and whether
/// that holds its removal back though nothing else does: where the name is
/// its own, or one it holds that a package kept or replaced depends on. Its removal goes
/// before the walk that places that package, at the latest
/// (`installed/a-package-losing-a-name-it-provides-is-removed-at-the-turn-of-its-successor`,
/// `installed/a-package-whose-own-name-another-takes-is-removed-at-its-turn`,
/// `installed/a-package-losing-a-name-nothing-else-needs-is-removed-first`,
/// `installed/a-displaced-package-goes-before-its-successor-s-walk`). A `!`
/// entry is no dependency here, as
/// [`Package::depends`](super::universe::Package::depends) leaves it out
/// (`installed/a-package-losing-a-name-a-kept-package-forbids-is-removed-first`),
/// and a name a condition names holds nothing back
/// (`installed/a-package-losing-a-name-a-kept-package-s-condition-names-is-removed-first`).
/// A name both provide at no version displaces too, which a removal left
/// for last shows
/// (`installed/a-removal-left-for-last-goes-before-a-package-providing-a-name-it-provides`);
/// a package providing a name at no version that the new one holds is not
/// held back by it, and goes first
/// (`installed/an-installed-provider-is-compared-first-bb-cc`).
fn successors(
    universe: &Universe<'_>,
    old: &[PackageId],
    new: &[PackageId],
    fates: &[Fate],
) -> Vec<Option<(usize, bool)>> {
    // The names the packages kept or replaced depend on.
    let needed: BTreeSet<NameId> = old
        .iter()
        .zip(fates)
        .filter(|&(_, &fate)| fate != Fate::Removed)
        .flat_map(|(&package, _)| universe.package(package).depends.iter())
        .map(|&(name, _)| name)
        .collect();
    // Whether `other` takes a name `one` holds, and whether that holds
    // `one`'s removal back.
    let clash = |one: PackageId, other: PackageId| {
        let (one, other) = (universe.package(one), universe.package(other));
        let taken = |held: NameId| other.candidacy(held).is_some();
        let held_back = taken(one.name)
            || one
                .holds()
                .any(|held| taken(held) && needed.contains(&held));
        (one.provides.iter().any(|&(name, _)| taken(name)), held_back)
    };
    old.iter()
        .zip(fates)
        .map(|(&removed, &fate)| {
            if fate != Fate::Removed {
                return None;
            }
            new.iter().enumerate().find_map(|(turn, &added)| {
                let (taken, held_back) = clash(removed, added);
                taken.then_some((turn, held_back))
            })
        })
        .collect()
}

/// What becomes of an installed package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Kept,
    Replaced,
    Removed,
}

/// A change to a package of the resolution, before it is ordered.
#[derive(Debug, Clone, Copy)]
enum Step<'a> {
    Install(&'a Record),
    /// Replaces the installed package at this index.
    Replace(usize, &'a Record),
}

/// One installed package's need of another, through a dependency or an
/// `install_if` entry.
#[derive(Debug, Clone, Copy)]
struct Need {
    /// The installed package needed.
    of: usize,
    /// Whether its removal follows at once when the package needing it
    /// changes: whether it carries the name
    /// (`installed/a-removal-waits-for-every-package-that-needed-it`).
    follows: bool,
    /// Whether it is needed through a name it provides at a version, which
    /// puts its removal after the packages installed
    /// (`installed/a-package-needed-through-a-name-it-provides-at-a-version-is-removed-last`).
    late: bool,
    /// Whether an `install_if` entry names it, and no dependency.
    condition: bool,
}

/// The removals, as they are ordered.
struct Order<'a, 'f> {
    installed: &'f [&'a Record],
    fates: &'f [Fate],
    /// For each installed package, the installed packages whose removal
    /// waits for its change, in the order it states its needs.
    needs: Vec<Vec<Need>>,
    /// For each installed package, the installed packages removed whose
    /// removal its removal waits for, in the order given.
    waiting_on: Vec<Vec<usize>>,
    /// For each installed package, the installed packages removed whose
    /// removal its removal waits for that it pulls, in the order given:
    /// those needing it through a name it provides at no version, or
    /// through a condition, which it says.
    dependents: Vec<Vec<(bool, usize)>>,
    /// For each installed package, how many changes its removal waits for
    /// have yet to come.
    pending: Vec<usize>,
    /// For each installed package, whether a package needs it through a
    /// name it provides at a version.
    late: Vec<bool>,
    removed: Vec<bool>,
    /// For each installed package, whether a walk of [`Order::pull`] is
    /// walking it: false between walks.
    active: Vec<bool>,
    /// For each installed package, whether a walk of [`Order::cascade`]
    /// has reached it: false between walks.
    seen: Vec<bool>,
    /// For each installed package, the place in apk's table of names of
    /// the first of the names it carries and provides, apk having read the
    /// world's names first
    /// (`installed/the-world-s-names-are-read-first-for-the-order-of-removals`).
    places: Vec<(u32, core::cmp::Reverse<usize>)>,
    changes: Vec<Change<'a>>,
}

impl<'a, 'f> Order<'a, 'f> {
    /// The order of the changes to `installed`, the packages `old` of
    /// `universe`, whose indexes `index_of` gives by their places in it,
    /// whose fates are `fates`, for a resolution holding `packages`, with
    /// the names in the order of `names`.
    fn new(
        universe: &Universe<'_>,
        old: &[PackageId],
        index_of: &[Option<usize>],
        installed: &'f [&'a Record],
        fates: &'f [Fate],
        packages: &[&Record],
        names: &Universe<'_>,
    ) -> Order<'a, 'f> {
        let places = old
            .iter()
            .map(|&id| {
                universe
                    .package(id)
                    .provides
                    .iter()
                    .map(|&(name, _)| names.position(universe.name(name).text))
                    .min()
                    .unwrap_or((u32::MAX, core::cmp::Reverse(0)))
            })
            .collect();
        // The names the resolution's packages carry or provide.
        let provided: BTreeSet<&Name> = packages
            .iter()
            .flat_map(|&record| {
                core::iter::once(record.name())
                    .chain(record.provides().iter().map(|provide| provide.name()))
            })
            .collect();
        let mut needs = vec![Vec::new(); installed.len()];
        let mut waits_for = vec![Vec::new(); installed.len()];
        let mut late = vec![false; installed.len()];
        let mut pullers: Vec<Vec<(bool, usize)>> = vec![Vec::new(); installed.len()];
        for (at, &id) in old.iter().enumerate() {
            let fate = fates.get(at).copied().unwrap_or(Fate::Kept);
            if fate == Fate::Kept {
                continue;
            }
            // What the package depends on, `!` entries left out, then the
            // entries of its condition but its `!` entries.
            let needing = universe.package(id);
            let entries = needing
                .depends
                .iter()
                .map(|&(name, entry)| (name, entry, false))
                .chain(
                    needing
                        .condition
                        .iter()
                        .map(|&(name, entry)| (name, entry, true)),
                )
                .filter(|&(_, entry, _)| !entry.is_conflict());
            let mut needed: Vec<Need> = Vec::new();
            // Where each package needed is in `needed`.
            let mut listed: BTreeMap<usize, usize> = BTreeMap::new();
            for (name, entry, condition) in entries {
                for (package, version) in universe.admitted(name, Some(entry)) {
                    let Some(other) = index_of.get(package.index()).copied().flatten() else {
                        continue;
                    };
                    if other == at {
                        continue;
                    }
                    let carried = universe.package(package).name == name;
                    // A package replaced waits for nothing it needed
                    // through a name provided at no version
                    // (`installed/a-provider-at-no-version-is-removed-before-a-replacement-of-what-needed-it`),
                    // nor through one the resolution still provides
                    // (`installed/a-provider-replaced-for-a-package-replaced-is-removed-first`).
                    let waits = fate == Fate::Removed
                        || carried
                        || (version.is_some() && !provided.contains(entry.name()));
                    if !waits {
                        continue;
                    }
                    let late = !carried && version.is_some();
                    // The last entry naming it says whether it is needed
                    // through a name provided at a version
                    // (`installed/a-package-needed-last-by-name-follows`).
                    match listed.get(&other).and_then(|&slot| needed.get_mut(slot)) {
                        Some(need) => {
                            need.follows |= carried;
                            need.late = late;
                            need.condition &= condition;
                        }
                        None => {
                            listed.insert(other, needed.len());
                            needed.push(Need {
                                of: other,
                                follows: carried,
                                late,
                                condition,
                            });
                        }
                    }
                }
            }
            for need in &mut needed {
                // Needed through a name provided at a version last, after
                // its own, a package does not follow
                // (`installed/a-package-needed-by-name-and-through-a-provided-name-is-removed-last`).
                need.follows &= !need.late;
                if let Some(list) = waits_for.get_mut(need.of) {
                    list.push(at);
                }
                // A removal pulls one waiting on it through a condition or
                // a name it provides at no version; one needing it by its
                // own name it waits for, to follow it.
                if fate == Fate::Removed
                    && (need.condition || !need.follows)
                    && let Some(list) = pullers.get_mut(need.of)
                {
                    list.push((need.condition, at));
                }
                if need.late
                    && let Some(slot) = late.get_mut(need.of)
                {
                    *slot = true;
                }
            }
            if let Some(slot) = needs.get_mut(at) {
                *slot = needed;
            }
        }
        let pending = waits_for.iter().map(Vec::len).collect();
        let dependents = pullers;
        let waiting_on = waits_for
            .iter()
            .map(|list| {
                let mut list: Vec<usize> = list
                    .iter()
                    .copied()
                    .filter(|&other| fates.get(other) == Some(&Fate::Removed))
                    .collect();
                list.sort_unstable();
                list.dedup();
                list
            })
            .collect();
        Order {
            installed,
            fates,
            needs,
            waiting_on,
            dependents,
            pending,
            late,
            removed: vec![false; installed.len()],
            active: vec![false; installed.len()],
            seen: vec![false; installed.len()],
            places,
            changes: Vec::new(),
        }
    }

    /// Whether the installed package at `at` is to be removed and has not
    /// been.
    fn waiting(&self, at: usize) -> bool {
        self.fates.get(at) == Some(&Fate::Removed) && self.removed.get(at) == Some(&false)
    }

    /// Whether a package needs the installed package at `at` through a name
    /// it provides at a version.
    fn late(&self, at: usize) -> bool {
        self.late.get(at) == Some(&true)
    }

    /// Whether the installed package at `at`, which a package of the
    /// resolution displaces, is to be removed and waits for no other
    /// removal: a package replaced that still needs it holds it up no
    /// longer.
    fn displaced_ready(&self, at: usize) -> bool {
        self.waiting(at)
            && self
                .waiting_on
                .get(at)
                .is_none_or(|list| list.iter().all(|&other| !self.waiting(other)))
    }

    /// Whether the installed package at `at` is to be removed, and waits
    /// for no other change.
    fn ready(&self, at: usize) -> bool {
        self.waiting(at) && self.pending.get(at) == Some(&0)
    }

    /// Removes the installed package at `at`, and then the removals that
    /// follow it.
    fn remove(&mut self, at: usize) {
        if self.waiting(at) {
            self.cascade(at, true);
        }
    }

    /// Removes the installed package at `root`, after the removals waiting
    /// on it that can go now, each pulled in the same way: those needing it
    /// through a name it provides at no version, then those through a
    /// condition, each in the order of apk's table of names, at the first
    /// of the names it carries and provides
    /// (`installed/a-removal-waiting-on-removals-pulls-them-first`,
    /// `installed/removals-a-removal-pulls-go-in-the-order-of-apk-s-table-of-names`,
    /// `installed/a-package-installed-for-its-condition-goes-after-a-package-needing-its-condition`).
    /// One needed through a name it provides at a version pulls nothing,
    /// and goes only where nothing waits on it already
    /// (`installed/a-package-freed-by-removals-goes-with-them-when-listed-after-them`);
    /// one a package of the resolution displaces is not pulled.
    fn pull(&mut self, root: usize, displaced: &dyn Fn(usize) -> bool) {
        if !self.waiting(root) {
            return;
        }
        // Each mark set below is cleared as its package leaves the stack.
        let mut active = core::mem::take(&mut self.active);
        let pulled = |order: &Self, at: usize| -> Vec<usize> {
            if order.late(at) {
                return Vec::new();
            }
            let mut list = order.dependents.get(at).cloned().unwrap_or_default();
            list.sort_by_key(|&(condition, other)| {
                (condition, order.places.get(other).copied(), other)
            });
            list.into_iter().map(|(_, other)| other).collect()
        };
        if let Some(mark) = active.get_mut(root) {
            *mark = true;
        }
        let mut stack: Vec<(usize, Vec<usize>, usize)> = vec![(root, pulled(self, root), 0)];
        while let Some((at, next, step)) = stack.last_mut() {
            let at = *at;
            if let Some(&other) = next.get(*step) {
                *step = step.saturating_add(1);
                if self.waiting(other)
                    && !displaced(other)
                    && let Some(mark) = active.get_mut(other)
                    && !core::mem::replace(mark, true)
                {
                    let next = pulled(self, other);
                    stack.push((other, next, 0));
                }
                continue;
            }
            stack.pop();
            if let Some(mark) = active.get_mut(at) {
                *mark = false;
            }
            if self.ready(at) {
                self.cascade(at, true);
            }
        }
        self.active = active;
    }

    /// Removes the installed package at `root`, where `with_root`, and each
    /// removal that follows it, in the reverse of the order a depth-first
    /// walk from it leaves them, through the needs each follows, taken last
    /// stated first: a package goes after every package of the walk that
    /// needed it, and of the packages one needs, the one it states first
    /// goes first (`installed/freed-packages-are-removed-depth-first`,
    /// `installed/freed-packages-are-removed-in-the-order-stated`,
    /// `installed/a-package-two-removals-free-goes-after-the-first-one-s-other-needs`).
    /// Each goes once nothing else waits on it; one that still waits stays
    /// for a later removal to free it.
    fn cascade(&mut self, root: usize, with_root: bool) {
        // A dependency's needs are walked last stated first, so that they
        // go in the order stated; a condition's in the order stated, so that
        // they go last stated first
        // (`installed/a-package-installed-for-its-condition-goes-after-a-package-needing-its-condition`).
        let followed = |order: &Self, at: usize| -> Vec<usize> {
            let needs = order.needs.get(at).map(Vec::as_slice).unwrap_or_default();
            let conditions = needs.iter().filter(|need| need.condition);
            let dependencies = needs.iter().rev().filter(|need| !need.condition);
            conditions
                .chain(dependencies)
                .filter(|need| need.follows)
                .map(|need| need.of)
                .collect()
        };
        let mut seen = core::mem::take(&mut self.seen);
        if let Some(mark) = seen.get_mut(root) {
            *mark = true;
        }
        let mut left: Vec<usize> = Vec::new();
        let mut stack: Vec<(usize, Vec<usize>, usize)> = vec![(root, followed(self, root), 0)];
        while let Some((at, next, step)) = stack.last_mut() {
            let Some(&other) = next.get(*step) else {
                left.push(*at);
                stack.pop();
                continue;
            };
            *step = step.saturating_add(1);
            if self.waiting(other)
                && let Some(mark) = seen.get_mut(other)
                && !core::mem::replace(mark, true)
            {
                let next = followed(self, other);
                stack.push((other, next, 0));
            }
        }
        // Every package marked reached the stack, and so `left`.
        for &at in &left {
            if let Some(mark) = seen.get_mut(at) {
                *mark = false;
            }
        }
        self.seen = seen;
        for at in left.into_iter().rev() {
            let go = if at == root {
                with_root && self.waiting(at)
            } else {
                self.ready(at)
            };
            if !go {
                continue;
            }
            if let Some(removed) = self.removed.get_mut(at) {
                *removed = true;
            }
            if let Some(&record) = self.installed.get(at) {
                self.changes.push(Change::Remove(record));
            }
            self.free(at);
        }
    }

    /// Removes the removals following the packages replaced of `freed`
    /// whose placement's walk has ended, as `ended` says of its end, the
    /// earliest replaced first.
    fn follow(&mut self, freed: &mut Vec<(usize, usize)>, ended: impl Fn(usize) -> bool) {
        let (done, waiting): (Vec<_>, Vec<_>) = core::mem::take(freed)
            .into_iter()
            .partition(|&(end, _)| ended(end));
        *freed = waiting;
        for (_, replaced) in done {
            self.cascade(replaced, false);
        }
    }

    /// Counts the change to the installed package at `at` for each package
    /// whose removal waits for it.
    fn free(&mut self, at: usize) {
        let needs = self.needs.get(at).cloned().unwrap_or_default();
        for need in needs {
            if let Some(count) = self.pending.get_mut(need.of) {
                *count = count.saturating_sub(1);
            }
        }
    }

    /// Removes the removals left, waiting on each other or needed through a
    /// name provided at a version
    /// (`installed/a-package-needed-through-a-name-it-provides-at-a-version-is-removed-last`),
    /// in the order given, each after the removals left waiting on it, which
    /// go first in the same way, in the order of apk's table of names, each
    /// at the first of the names it carries and provides
    /// (`installed/the-last-removals-go-in-the-order-of-apk-s-table-of-names`,
    /// `reduced/google-cloud-cpp-s-cycle-is-removed-protobuf-internal-first`);
    /// where removals wait on each other, the one reached last goes first
    /// (`installed/removals-in-a-cycle-come-last`).
    /// After each, the removals it frees that carry a name it needs, the one
    /// given last first
    /// (`installed/the-last-removals-a-removal-frees-go-the-one-given-last-first`).
    fn last(&mut self) {
        let mut active = vec![false; self.installed.len()];
        for root in 0..self.installed.len() {
            self.finish(root, &mut active);
        }
    }

    /// Removes the installed package at `at`, if it is still to be removed,
    /// as [`Order::last`] removes each: after the removals still waiting on
    /// it, and with those it frees.
    fn reach(&mut self, at: usize) {
        let mut active = vec![false; self.installed.len()];
        self.finish(at, &mut active);
    }

    /// Removes the installed package at `root`, if it is still to be
    /// removed, after the removals waiting on it, in the order of apk's
    /// table of names, each with the removals it frees; `active` marks the
    /// packages being walked, none between calls.
    fn finish(&mut self, root: usize, active: &mut [bool]) {
        let waiting_on = |order: &Self, at: usize| -> Vec<usize> {
            let mut list: Vec<usize> = order
                .waiting_on
                .get(at)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&other| order.waiting(other))
                .collect();
            list.sort_by_key(|&other| (order.places.get(other).copied(), other));
            list
        };
        if !self.waiting(root) {
            return;
        }
        if let Some(mark) = active.get_mut(root) {
            *mark = true;
        }
        let mut stack: Vec<(usize, Vec<usize>, usize)> = vec![(root, waiting_on(self, root), 0)];
        while let Some((at, next, step)) = stack.last_mut() {
            let at = *at;
            if let Some(&other) = next.get(*step) {
                *step = step.saturating_add(1);
                if self.waiting(other)
                    && let Some(mark) = active.get_mut(other)
                    && !core::mem::replace(mark, true)
                {
                    let next = waiting_on(self, other);
                    stack.push((other, next, 0));
                }
                continue;
            }
            stack.pop();
            if let Some(mark) = active.get_mut(at) {
                *mark = false;
            }
            self.take_with_followers(at, active);
        }
    }

    /// Removes the installed package at `at`, and then the removals it
    /// frees that carry a name it needs and wait on nothing else, outside
    /// `active`, each with its own, the one given last first.
    fn take_with_followers(&mut self, at: usize, active: &[bool]) {
        let mut stack = vec![at];
        while let Some(at) = stack.pop() {
            if !self.waiting(at) {
                continue;
            }
            self.take(at);
            let mut freed: Vec<usize> = self
                .needs
                .get(at)
                .into_iter()
                .flatten()
                .filter(|need| need.follows)
                .map(|need| need.of)
                .filter(|&other| self.ready(other) && active.get(other) != Some(&true))
                .collect();
            freed.sort_unstable();
            freed.dedup();
            stack.extend(freed);
        }
    }

    /// Removes the installed package at `at` if it is still to be removed,
    /// whatever waits on it, and nothing with it.
    fn take(&mut self, at: usize) {
        if !self.waiting(at) {
            return;
        }
        if let Some(removed) = self.removed.get_mut(at) {
            *removed = true;
        }
        if let Some(&record) = self.installed.get(at) {
            self.changes.push(Change::Remove(record));
        }
        self.free(at);
    }
}
