//! The records a resolver holds, indexed for resolving: every package, every
//! name, and the edges between them.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::cmp::{Ordering, Reverse};

use super::table;
use crate::dependency::Dependency;
use crate::record::{Identity, Record};
use crate::version::Version;

/// A package's place in [`Universe::packages`], which is the order records
/// were added in: the order that breaks a tie between candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PackageId(usize);

/// A candidate for a name: a package, and the version it is one at.
pub(super) type Candidate<'v> = (PackageId, Option<&'v Version>);

/// A name's place in [`Universe::names`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NameId(usize);

impl PackageId {
    pub(super) fn index(self) -> usize {
        self.0
    }
}

impl NameId {
    pub(super) fn index(self) -> usize {
        self.0
    }
}

/// Every package a resolver may install, and every name one carries,
/// provides, or depends on.
#[derive(Debug, Clone, Default)]
pub(super) struct Universe<'a> {
    packages: Vec<Package<'a>>,
    names: Vec<Name<'a>>,
    ids: BTreeMap<&'a [u8], NameId>,
    /// Every identity a record came with, and the package it is: none
    /// where apk could not install the record that came with it first.
    identities: BTreeMap<&'a Identity, Option<PackageId>>,
    /// Every name the records and the world mention, with its place in the
    /// order apk reads names in (see [`Universe::position`]).
    read: BTreeMap<&'a [u8], usize>,
    /// Every package installed in the root, those apk cannot install
    /// included, by its identity, with the root's record of it.
    held: BTreeMap<&'a Identity, &'a Record>,
}

/// One installable package.
#[derive(Debug, Clone)]
pub(super) struct Package<'a> {
    pub(super) record: &'a Record,
    /// The name the package carries.
    pub(super) name: NameId,
    /// Each name the package is a candidate for, and the version it is one
    /// at: its own name at its own version first, then each name it
    /// provides, in the order stated. A name appears once, at its first
    /// mention.
    pub(super) provides: Vec<(NameId, Option<&'a Version>)>,
    /// What the package depends on, in the order stated. Conflicts are left
    /// out.
    pub(super) depends: Vec<(NameId, &'a Dependency)>,
    /// The `!` entries of what the package depends on: what may not be
    /// installed with it.
    pub(super) conflicts: Vec<(NameId, &'a Dependency)>,
    /// The package's `install_if` condition, in the order stated, `!`
    /// entries included. Empty for a package installed only when required.
    pub(super) condition: Vec<(NameId, &'a Dependency)>,
    /// Whether the package is installed in the root already.
    pub(super) installed: bool,
    /// Whether the package conflicts with itself: apk chooses it as any
    /// other, and refuses the request when it does
    /// (`providers/a-package-conflicting-with-itself-is-still-chosen`).
    pub(super) conflicts_with_itself: bool,
    /// Whether a `!` entry of the package's condition names a name the
    /// package holds, its own or one it provides at a version, whatever
    /// version the entry admits: the condition never holds
    /// (`install-if/a-condition-forbidding-the-package-s-own-name-never-holds`).
    pub(super) condition_forbids_itself: bool,
}

impl<'a> Package<'a> {
    /// The names the package holds, which no other package may provide
    /// beside it: its own, and each it provides at a version.
    pub(super) fn holds(&self) -> impl Iterator<Item = NameId> + '_ {
        self.provides
            .iter()
            .filter(|(_, version)| version.is_some())
            .map(|&(name, _)| name)
    }

    /// The version the package is a candidate for `name` at, if it is a
    /// candidate for it: `Some(None)` where it provides the name at no
    /// version.
    pub(super) fn candidacy(&self, name: NameId) -> Option<Option<&'a Version>> {
        self.provides
            .iter()
            .find(|&&(provided, _)| provided == name)
            .map(|&(_, version)| version)
    }
}

/// One name.
#[derive(Debug, Clone)]
pub(super) struct Name<'a> {
    pub(super) text: &'a [u8],
    /// Every package that is a candidate for the name, in the order added,
    /// with the version it is one at.
    pub(super) providers: Vec<(PackageId, Option<&'a Version>)>,
    /// Whether some record carries the name as its own, installable or not.
    pub(super) real: bool,
    /// Every entry naming the name among a package's dependencies: the
    /// package, and the entry's place in its [`Package::depends`], in the
    /// order added and, within a package, the order stated. A package with
    /// several entries on the name stands here once for each.
    pub(super) dependents: Vec<(PackageId, usize)>,
    /// Every package whose `install_if` condition names the name, in the
    /// order added.
    pub(super) triggers: Vec<PackageId>,
}

impl Name<'_> {
    /// Every package that depends on the name, once each, in the order
    /// added.
    pub(super) fn dependent_packages(&self) -> impl Iterator<Item = PackageId> + '_ {
        let mut last = None;
        self.dependents.iter().filter_map(move |&(package, _)| {
            (last.replace(package) != Some(package)).then_some(package)
        })
    }
}

impl<'a> Universe<'a> {
    /// Notes the names of a root's world, which apk reads before any record
    /// (`installed/the-world-s-names-are-read-before-the-installed-database-s`).
    pub(super) fn note_world(&mut self, world: &[&'a Dependency]) {
        for entry in world {
            self.note(entry.name().as_bytes());
        }
    }

    /// Adds `record`, unless apk cannot install it, and reports the
    /// package. A record that conflicts with itself is a package all the
    /// same. A record with the identity of one added before is that
    /// record's package, whatever its name, or none where apk could not
    /// install that record
    /// (`repositories/a-record-with-the-identity-of-another-name-is-no-package`,
    /// `repositories/a-record-with-the-identity-of-a-record-apk-cannot-install-is-that-record`).
    /// Its name is known to be real either way.
    pub(super) fn add(&mut self, record: &'a Record) -> Option<PackageId> {
        let claimed = self.identities.get(record.identity()) == Some(&None);
        if record.is_readable() && !claimed {
            Some(self.insert(record))
        } else {
            self.note_unadded(record);
            self.identities.entry(record.identity()).or_insert(None);
            None
        }
    }

    /// Notes the names of `record`, a record not added as a package; its
    /// own name is real all the same.
    fn note_unadded(&mut self, record: &'a Record) {
        self.note_read(record);
        let name = self.intern(record.name().as_bytes());
        if let Some(entry) = self.names.get_mut(name.index()) {
            entry.real = true;
        }
    }

    /// Adds `record`, a package installed in the root, unless apk cannot
    /// install it, conflicting with itself included: an installed package
    /// apk could not install again is passed over as any other is
    /// (`installed/an-installed-package-apk-cannot-install-is-replaced`).
    pub(super) fn add_installed(&mut self, record: &'a Record) {
        self.held.entry(record.identity()).or_insert(record);
        if !record.is_installable() {
            self.note_unadded(record);
            self.identities.entry(record.identity()).or_insert(None);
            return;
        }
        if let Some(id) = self.add(record)
            && let Some(package) = self.packages.get_mut(id.index())
        {
            package.installed = true;
        }
    }

    /// Adds `record` whether apk could install it or not, and returns its
    /// package: the one added before with its identity, whatever its name,
    /// where there is one.
    pub(super) fn insert(&mut self, record: &'a Record) -> PackageId {
        self.note_read(record);
        let name = self.intern(record.name().as_bytes());
        if let Some(entry) = self.names.get_mut(name.index()) {
            entry.real = true;
        }
        let id = PackageId(self.packages.len());
        if let Some(&Some(known)) = self.identities.get(record.identity()) {
            return known;
        }
        self.identities.insert(record.identity(), Some(id));

        let mut provides: Vec<(NameId, Option<&'a Version>)> = Vec::new();
        for (text, version) in record.candidacies() {
            let provided = self.intern(text.as_bytes());
            if provides.iter().all(|(known, _)| *known != provided) {
                provides.push((provided, version));
                if let Some(entry) = self.names.get_mut(provided.index()) {
                    entry.providers.push((id, version));
                }
            }
        }

        let mut depends = Vec::new();
        let mut conflicts = Vec::new();
        for entry in record.depends() {
            let needed = self.intern(entry.name().as_bytes());
            if entry.is_conflict() {
                conflicts.push((needed, entry));
                continue;
            }
            if let Some(name) = self.names.get_mut(needed.index()) {
                name.dependents.push((id, depends.len()));
            }
            depends.push((needed, entry));
        }

        let mut condition = Vec::new();
        for entry in record.install_if() {
            let named = self.intern(entry.name().as_bytes());
            condition.push((named, entry));
            if let Some(name) = self.names.get_mut(named.index())
                && name.triggers.last() != Some(&id)
            {
                name.triggers.push(id);
            }
        }

        let mut package = Package {
            record,
            name,
            provides,
            depends,
            conflicts,
            condition,
            installed: false,
            conflicts_with_itself: record.conflicts_with_itself(),
            condition_forbids_itself: false,
        };
        package.condition_forbids_itself = package.condition.iter().any(|&(named, entry)| {
            entry.is_conflict() && package.holds().any(|held| held == named)
        });
        self.packages.push(package);
        id
    }

    /// Notes the names `record` mentions, in the order apk reads them: its
    /// fields in the order they stand, and a field's entries in the order
    /// stated, whether apk can install the record or not
    /// (`install-order/a-name-is-read-where-the-index-first-mentions-it-xp-mentioned-first`,
    /// `install-order/a-record-s-names-are-read-in-the-order-its-fields-stand`).
    /// Each kind of list is taken whole where its letter first stands: the
    /// order of the text wherever a record states each list on one line, as
    /// every record of Alpine's indexes does. An entry ferrata cannot read is
    /// not noted.
    fn note_read(&mut self, record: &'a Record) {
        let mut letters: Vec<u8> = Vec::new();
        for (letter, _) in record.fields() {
            if matches!(letter, b'P' | b'D' | b'p' | b'i') && !letters.contains(&letter) {
                letters.push(letter);
            }
        }
        for letter in letters {
            match letter {
                b'P' => self.note(record.name().as_bytes()),
                b'D' => record
                    .depends()
                    .iter()
                    .for_each(|entry| self.note(entry.name().as_bytes())),
                b'p' => record
                    .provides()
                    .iter()
                    .for_each(|provide| self.note(provide.name().as_bytes())),
                _ => record
                    .install_if()
                    .iter()
                    .for_each(|entry| self.note(entry.name().as_bytes())),
            }
        }
    }

    /// Notes that apk has read `name`, unless it read it before.
    fn note(&mut self, name: &'a [u8]) {
        let next = self.read.len();
        self.read.entry(name).or_insert(next);
    }

    /// Where apk's table of names holds `name` (see [`table`]), given its
    /// place in the order apk read names in, the world's first: a name that
    /// no record mentions comes after every one that does, so first in its
    /// bucket.
    pub(super) fn position(&self, name: &[u8]) -> (u32, Reverse<usize>) {
        let read = self.read.get(name).copied().unwrap_or(usize::MAX);
        table::position(name, read)
    }

    fn intern(&mut self, text: &'a [u8]) -> NameId {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        let id = NameId(self.names.len());
        self.names.push(Name {
            text,
            providers: Vec::new(),
            real: false,
            dependents: Vec::new(),
            triggers: Vec::new(),
        });
        self.ids.insert(text, id);
        id
    }

    /// Whether `entry`, on `name`, admits `package` as a candidate for
    /// `name` at `version`, as [`Record::answers`] says: an entry naming an
    /// identity admits the package of that name with that identity, one with
    /// no constraint admits any candidate, and one with a constraint admits
    /// a candidate at a version it admits, and one at no version where it
    /// admits an earlier version
    /// (`providers/a-provider-at-no-version-answers-an-entry-admitting-earlier-versions`).
    /// No entry admits every candidate.
    pub(super) fn admits(
        &self,
        entry: Option<&Dependency>,
        name: NameId,
        package: PackageId,
        version: Option<&Version>,
    ) -> bool {
        let candidate = self.package(package);
        candidate
            .record
            .admitted(entry, version, candidate.name == name)
    }

    /// The candidates for `name` that `entry` admits (see
    /// [`Universe::admits`]), in the order of the name's providers; with no
    /// entry, every candidate. Whether a `!` entry's candidates are what it
    /// forbids or what it leaves alone is for the caller to say.
    pub(super) fn admitted<'s>(
        &'s self,
        name: NameId,
        entry: Option<&'s Dependency>,
    ) -> impl Iterator<Item = Candidate<'a>> + 's {
        self.name(name)
            .providers
            .iter()
            .copied()
            .filter(move |&(package, version)| self.admits(entry, name, package, version))
    }

    /// Whether `package` is a candidate for `name` that `entry` admits, at
    /// the version it is one at: [`Record::answers`] over the universe's
    /// names.
    pub(super) fn answers(
        &self,
        package: PackageId,
        name: NameId,
        entry: Option<&Dependency>,
    ) -> bool {
        self.package(package)
            .candidacy(name)
            .is_some_and(|version| self.admits(entry, name, package, version))
    }

    /// Whether some package is a candidate for `name` that `entry` admits:
    /// whether any package could ever meet it.
    pub(super) fn offers(&self, name: NameId, entry: &Dependency) -> bool {
        self.admitted(name, Some(entry)).next().is_some()
    }

    /// Whether `a` and `b` are two packages one of which holds a name (see
    /// [`Package::holds`]) the other provides, at a version or at none:
    /// they cannot be installed together. This is the one statement of the
    /// rule; the solver applies it name by name as it installs a package
    /// (`Solve::claim`), and the preliminary choices through an index of
    /// what each choice holds and provides (`Choices::against`).
    pub(super) fn holds_against(&self, a: PackageId, b: PackageId) -> bool {
        let clash = |x: PackageId, y: PackageId| {
            self.package(x)
                .holds()
                .any(|held| self.package(y).candidacy(held).is_some())
        };
        a != b && (clash(a, b) || clash(b, a))
    }

    /// Whether apk may choose `candidate`: a provider at no version only
    /// with a priority above 0
    /// (`providers/a-name-provided-at-no-version-needs-a-priority`), installed
    /// or not (`installed/an-installed-provider-at-no-version-needs-a-priority`).
    pub(super) fn choosable(&self, (package, version): Candidate<'_>) -> bool {
        version.is_some() || self.package(package).record.priority() > 0
    }

    /// Whether apk installs `challenger` rather than `holder`, two candidates
    /// for one name. Two versions of one package, candidates at one version
    /// or both at none, go by which is installed, then by their own
    /// version, whatever their priorities
    /// (`providers/of-two-versions-providing-a-name-alike-the-later-is-chosen`,
    /// `providers/of-two-versions-providing-a-name-at-no-version-the-later-is-chosen`,
    /// `installed/of-two-versions-providing-a-name-alike-the-installed-is-kept`).
    /// Two providers at no version go by priority; failing
    /// that, a package installed already is kept
    /// (`installed/an-installed-package-is-kept-though-a-later-version-is-offered`,
    /// `installed/an-installed-provider-outranks-a-higher-priority-at-a-version`,
    /// `installed/a-higher-priority-outranks-an-installed-provider-at-no-version`);
    /// failing that, a provider at a version outranks one at none, then the
    /// later version, then the higher priority. Equal candidates are no
    /// reason to change, so the one taken first stays.
    ///
    /// With a package installed, or two versions of one package alike, the
    /// relation is not an order: of an installed provider at no version, a
    /// higher priority at no version and a provider at a version, each
    /// outranks the next and the last the first; and of `c` 1, `d` 1 and
    /// `c` 2 providing a name at one version, `c` 1 and `d` 1 tie, `d` 1 and
    /// `c` 2 tie, and `c` 2 outranks `c` 1. apk takes candidates in turn,
    /// and [`Universe::best`] does the same
    /// (`installed/an-installed-provider-is-compared-first-bb-cc`,
    /// `providers/of-two-versions-providing-a-name-alike-the-later-is-chosen-in-turn`).
    pub(super) fn outranks(&self, challenger: Candidate<'_>, holder: Candidate<'_>) -> bool {
        let (new, old) = (self.package(challenger.0), self.package(holder.0));
        if new.name == old.name && challenger.1.cmp(&holder.1) == Ordering::Equal {
            if new.installed != old.installed {
                return new.installed;
            }
            match new.record.version().cmp(old.record.version()) {
                Ordering::Equal => {}
                order => return order == Ordering::Greater,
            }
        }
        let (new_priority, old_priority) = (new.record.priority(), old.record.priority());
        if challenger.1.is_none() && holder.1.is_none() && new_priority != old_priority {
            return new_priority > old_priority;
        }
        if new.installed != old.installed {
            return new.installed;
        }
        (challenger.1.is_some(), challenger.1, new_priority)
            > (holder.1.is_some(), holder.1, old_priority)
    }

    /// The candidate apk chooses of `candidates`, taken in the order given:
    /// the order of a name's providers, the packages installed first, then
    /// the records of each repository in turn
    /// (`providers/equal-priorities-go-to-the-record-listed-first-p1-first`).
    /// Whether apk may choose each is for the caller to ask.
    pub(super) fn best<'v>(
        &self,
        candidates: impl IntoIterator<Item = Candidate<'v>>,
    ) -> Option<Candidate<'v>> {
        candidates
            .into_iter()
            .fold(None, |best, candidate| match best {
                Some(best) if !self.outranks(candidate, best) => Some(best),
                _ => Some(candidate),
            })
    }

    /// The candidates of `candidates` apk may choose, best first: as
    /// [`Universe::best`] would take them one after another.
    pub(super) fn ranked<'v>(&self, mut candidates: Vec<Candidate<'v>>) -> Vec<Candidate<'v>> {
        candidates.retain(|&candidate| self.choosable(candidate));
        let mut versions_of_one = BTreeSet::new();
        let alike = candidates.iter().any(|&(package, version)| {
            !versions_of_one.insert((self.package(package).name, version))
        });
        if !alike
            && candidates
                .iter()
                .all(|&(package, _)| !self.package(package).installed)
        {
            // Without a package installed or two versions of one package
            // alike, `outranks` is an order, and sorting by it takes the
            // candidates as `best` would.
            candidates.sort_by_key(|&(package, version)| {
                core::cmp::Reverse((
                    version.is_some(),
                    version,
                    self.package(package).record.priority(),
                    core::cmp::Reverse(package),
                ))
            });
            return candidates;
        }
        let mut ranked = Vec::with_capacity(candidates.len());
        while let Some(best) = self.best(candidates.iter().copied()) {
            ranked.push(best);
            candidates.retain(|&(package, _)| package != best.0);
        }
        ranked
    }

    /// The root's record of `package`, if the root held it: a package of
    /// its identity is installed there, whether or not apk can
    /// install the record the root has of it. This is what a root keeps or
    /// drops, and the record its changes name; it is not what ranks a
    /// candidate as installed, which is [`Package::installed`].
    pub(super) fn held(&self, package: PackageId) -> Option<&'a Record> {
        self.held
            .get(self.package(package).record.identity())
            .copied()
    }

    /// The packages the root held (see [`Universe::held`]), in the order
    /// added.
    pub(super) fn held_packages(&self) -> impl Iterator<Item = PackageId> + '_ {
        let count = if self.held.is_empty() {
            0
        } else {
            self.packages.len()
        };
        (0..count)
            .map(PackageId)
            .filter(|&package| self.held(package).is_some())
    }

    /// The package `record` is, if it is one: the one of its identity.
    pub(super) fn package_of(&self, record: &Record) -> Option<PackageId> {
        self.identities.get(record.identity()).copied().flatten()
    }

    /// The name `text`, if any record mentions it.
    pub(super) fn id(&self, text: impl AsRef<[u8]>) -> Option<NameId> {
        self.ids.get(text.as_ref()).copied()
    }

    pub(super) fn package_count(&self) -> usize {
        self.packages.len()
    }

    pub(super) fn name_count(&self) -> usize {
        self.names.len()
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "a `PackageId` is made only by `add`, as the index of the package it pushes"
    )]
    pub(super) fn package(&self, id: PackageId) -> &Package<'a> {
        &self.packages[id.0]
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "a `NameId` is made only by `intern`, as the index of the name it pushes"
    )]
    pub(super) fn name(&self, id: NameId) -> &Name<'a> {
        &self.names[id.0]
    }
}
