//! Explanations: why a package is in a resolution or out of it, as a
//! record of the steps that led there.
//!
//! The solver notes, as it goes, the requirement each package was installed
//! for, or its `install_if` condition, and the first thing that ruled each
//! package out. Each step of an explanation is one of those notes, and
//! points back at what came before it: the package or name a requirement
//! came from, the packages a condition is met by, the package or entry that
//! ruled a package out. Nothing points forward in time, so the steps form no
//! cycle, and every chain of them ends at the request or the world.

use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use super::{Package, RequiredBy};
use crate::Name;
use crate::dependency::Dependency;
use crate::record::Record;

/// A requirement on a name: the entry that stated it, and what stated it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    name: Name,
    entry: Option<Dependency>,
    by: RequiredBy,
}

impl Requirement {
    pub(super) fn new(name: Name, entry: Option<Dependency>, by: RequiredBy) -> Requirement {
        Requirement { name, entry, by }
    }

    /// The name required.
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The entry that stated the requirement, if one did: a name every
    /// candidate for another depends on is required without one (see
    /// [`RequiredBy::Candidates`]), unless there is a single candidate,
    /// whose first entry on the name states it (see
    /// [`RequiredBy::Package`]). For a `!` entry, the requirement is that
    /// nothing it admits be installed.
    pub fn entry(&self) -> Option<&Dependency> {
        self.entry.as_ref()
    }

    /// What stated the requirement.
    pub fn by(&self) -> &RequiredBy {
        &self.by
    }

    /// Whether the package `record` describes answers the requirement: it
    /// carries the name or provides it, as the
    /// [entry](Requirement::entry) admits, or at any version where no entry
    /// states the requirement. The rule is the one the resolver follows, as
    /// [`Record::answers`] says it of an entry. For a `!` entry, this says
    /// whether the entry forbids the package.
    ///
    /// ```
    /// use ferrata::{Dialect, index};
    /// use ferrata::resolve::{RequiredBy, Request, Resolver};
    ///
    /// let text = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:app\nV:1\nD:v\n\n\
    ///             C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:p1\nV:1\np:v\nk:10\nD:lib\n\n\
    ///             C:Q1CCCCCCCCCCCCCCCCCCCCCCCCCCC=\nP:p2\nV:1\np:v\nk:20\nD:lib\n\n\
    ///             C:Q1DDDDDDDDDDDDDDDDDDDDDDDDDDD=\nP:lib\nV:1\nX:1\n\n\
    ///             C:Q1EEEEEEEEEEEEEEEEEEEEEEEEEEE=\nP:zlib\nV:1\np:lib\n";
    /// let records: Vec<_> = index::read(Dialect::V3_0, text)
    ///     .map(|located| located.map(index::Located::into_record))
    ///     .collect::<Result<_, _>>()?;
    /// // Both candidates for `v` depend on `lib`, which is required with no
    /// // entry. apk cannot install the record of `lib`, and `zlib`, which
    /// // provides it, is withheld, so nothing answers it.
    /// let resolver = Resolver::new(Dialect::V3_0).repository(&records[..4]);
    /// let request: Request = [Dialect::V3_0.dependency("app")?].into_iter().collect();
    /// let refusal = resolver.resolve(&request).unwrap_err();
    /// let requirement = refusal.requirement();
    /// assert_eq!(requirement.by(), &RequiredBy::Candidates("v".into()));
    /// assert!(requirement.entry().is_none());
    /// assert!(requirement.answered_by(&records[4]));
    /// assert!(!requirement.answered_by(&records[1]));
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub fn answered_by(&self, record: &Record) -> bool {
        record.answers_on(&self.name, self.entry.as_ref())
    }

    /// Writes the entry, or the name where no entry stated it.
    fn write_entry(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.entry {
            Some(entry) => write!(f, "{entry}"),
            None => write!(f, "{}", self.name),
        }
    }
}

impl fmt::Display for Requirement {
    /// Writes the entry and what stated it: `lib>=2, required by app-1.0`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_entry(f)?;
        write!(f, ", required by {}", self.by)
    }
}

/// Why a package is in a resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// It was chosen for a name required, which it carries or provides: of
    /// the requirements on the name when it was chosen, the first an entry
    /// stated, or failing that, the first. A name every candidate for
    /// another depends on is required before that other name is decided,
    /// and the package then installed for it states the entry that says why.
    Required(Requirement),
    /// Its `install_if` condition holds: for each entry of the condition, in
    /// the order stated, the package installed first that answers it; for a
    /// `!` entry, the package of the name it names installed first, before
    /// this one, that the entry does not forbid. A `!` entry that holds with
    /// no package of its name installed names none, so a condition of such
    /// entries alone names no package: the package is there because its
    /// condition holds, and nothing else.
    Condition(Vec<Package>),
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::Required(requirement) => write!(f, "for {requirement}"),
            Reason::Condition(met_by) if met_by.is_empty() => f.write_str("for its condition"),
            Reason::Condition(met_by) => {
                f.write_str("for its condition, met by ")?;
                write_list(f, met_by)
            }
        }
    }
}

/// Why a package is not in a resolution: why apk passed it over, or, for a
/// package the root held before, why the resolution drops it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exclusion {
    /// A requirement on a name the package carries or provides does not
    /// admit it.
    NotAdmitted(Requirement),
    /// A `!` entry admits it: the requirement is the entry, and what stated
    /// it.
    Forbidden(Requirement),
    /// It and `holder` cannot both be installed: one of them carries `name`,
    /// or provides it at a version, and the other provides it too. The
    /// holder is a package installed, or one a [`Step::Sole`] names.
    Held {
        /// The name both provide.
        name: Name,
        /// The package holding it, or providing it beside the package.
        holder: Package,
    },
    /// Its dependency `entry` does not admit `holder`, which holds the name
    /// the entry names: a package installed, or one a [`Step::Sole`] names.
    Blocked {
        /// The dependency.
        entry: Dependency,
        /// The package holding its name.
        holder: Package,
    },
    /// Its dependency admits only packages ruled out already.
    Stranded(Dependency),
    /// apk cannot install its record (see
    /// [`Record::is_installable`](crate::record::Record::is_installable)).
    Uninstallable,
    /// Nothing requires it: no requirement the world and the request bring
    /// chose it. Only a package the root held before is dropped for this.
    Unrequired,
    /// Its `install_if` condition does not hold. Only a package the root
    /// held before is dropped for this. An explanation goes on to the
    /// packages an entry of the condition admits that were ruled out, and
    /// the packages installed a `!` entry of it admits.
    Condition,
    /// Its `install_if` condition held, and it could not be installed: its
    /// dependency `entry` admits no package that could be, or `entry`, one
    /// of its `!` entries, forbids a package installed. It depends on a
    /// name `completing`, a package completing the condition, provides, or
    /// forbids `completing`, and apk passes such a package over where
    /// another would refuse the request.
    Passed {
        /// The dependency or `!` entry it could not be installed for.
        entry: Dependency,
        /// The package completing its condition that it is tied to.
        completing: Package,
    },
    /// Its dependency admits no package installed or still open. apk
    /// passes such a candidate over, rather than choose it and refuse,
    /// where a candidate for the same name depends on a package up the
    /// chain of requirements that brought the name, or on one installed
    /// for the request or the world.
    Unmeetable(Dependency),
    /// Its `install_if` condition held, and its dependency admits only
    /// packages holding a name a `!` entry of the condition names, which
    /// apk keeps out of the package's own dependencies.
    Negated(Dependency),
}

impl fmt::Display for Exclusion {
    /// Writes the reason as a clause about the package, called "it".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exclusion::NotAdmitted(requirement) => write!(f, "{requirement}, does not admit it"),
            Exclusion::Forbidden(requirement) => {
                requirement.write_entry(f)?;
                write!(f, ", stated by {}, forbids it", requirement.by)
            }
            Exclusion::Held { name, holder } => {
                write!(f, "it and {holder} cannot both provide {name}")
            }
            Exclusion::Blocked { entry, holder } => write!(
                f,
                "its dependency {entry} does not admit {holder}, which holds the name"
            ),
            Exclusion::Stranded(entry) => {
                write!(f, "its dependency {entry} admits only packages ruled out")
            }
            Exclusion::Uninstallable => f.write_str("apk cannot install its record"),
            Exclusion::Unrequired => f.write_str("nothing requires it"),
            Exclusion::Condition => f.write_str("its condition does not hold"),
            Exclusion::Negated(entry) => write!(
                f,
                "its dependency {entry} admits only packages its condition forbids"
            ),
            Exclusion::Unmeetable(entry) => write!(
                f,
                "its dependency {entry} admits nothing that could be installed"
            ),
            Exclusion::Passed { entry, completing } if entry.is_conflict() => write!(
                f,
                "its condition holds through {completing}, which it is tied to, and its {entry} forbids a package installed"
            ),
            Exclusion::Passed { entry, completing } => write!(
                f,
                "its condition holds through {completing}, which it is tied to, and its dependency {entry} admits nothing that could be installed"
            ),
        }
    }
}

/// One step of an [`Explanation`]: one package or name, and why it stands
/// where it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A package installed, which the root did not hold before.
    Installed {
        /// The package.
        package: Package,
        /// Why it is installed.
        reason: Reason,
    },
    /// A package the root held before, which it keeps.
    Kept {
        /// The package.
        package: Package,
        /// Why it is kept: the same reasons a package is installed for.
        reason: Reason,
    },
    /// A name required, and what required it: a name whose every candidate
    /// depends on the name another step was required for, by
    /// [`RequiredBy::Candidates`].
    Required(Requirement),
    /// A package as it stood before it was chosen, if it was: the only
    /// candidate left for a name required, which holds what it would hold,
    /// rules out what it excludes, and requires what it depends on.
    Sole {
        /// The package.
        package: Package,
        /// The requirement on the name it is the only candidate for.
        requirement: Requirement,
    },
    /// A package the root did not hold, which apk passed over.
    RuledOut {
        /// The package.
        package: Package,
        /// Why it was passed over.
        exclusion: Exclusion,
    },
    /// A package the root held before, which the resolution drops: removed,
    /// or replaced by another package of its name.
    Dropped {
        /// The package.
        package: Package,
        /// Why it is dropped.
        exclusion: Exclusion,
    },
}

impl fmt::Display for Step {
    /// Writes the step as a sentence: `lib-2.0 is installed for lib>=2,
    /// required by app-1.0`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::Installed { package, reason } => write!(f, "{package} is installed {reason}"),
            Step::Kept { package, reason } => write!(f, "{package} is kept {reason}"),
            Step::Required(requirement) => write!(f, "{requirement}"),
            Step::Sole {
                package,
                requirement,
            } => write!(f, "{package} is the only candidate left for {requirement}"),
            Step::RuledOut { package, exclusion } => {
                write!(f, "{package} is ruled out: {exclusion}")
            }
            Step::Dropped { package, exclusion } => {
                write!(f, "{package} is dropped: {exclusion}")
            }
        }
    }
}

/// Why packages stand where they do in a resolution, or in the attempt at
/// one that apk refused: a list of [`Step`]s.
///
/// The first step is about the package asked about, or for a refusal, about
/// what stated the requirement that could not be met. Each step names what
/// it follows from: the package or the name a requirement came from, the
/// packages a condition is met by, the package or entry that ruled a
/// package out. Every step named is in the list, once, and after each step
/// that names it, so the list has no cycle: reading on from any step leads
/// to a requirement of the request or of the world. A step whose requirement
/// came from the request or the world names nothing further.
///
/// ```
/// use ferrata::Dialect;
/// use ferrata::index;
/// use ferrata::resolve::{Request, Resolver};
///
/// let text = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:app\nV:1.0\nD:so:libz.so.1\n\n\
///             C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:zlib\nV:1.3\np:so:libz.so.1=1.3\n";
/// let records: Vec<_> = index::read(Dialect::V3_0, text)
///     .map(|located| located.map(index::Located::into_record))
///     .collect::<Result<_, _>>()?;
/// let resolver = Resolver::new(Dialect::V3_0).repository(&records);
/// let request: Request = [Dialect::V3_0.dependency("app")?].into_iter().collect();
/// let resolution = resolver.resolve(&request)?;
/// let explanation = resolution.explain(&records[1]).expect("zlib is installed");
/// assert_eq!(
///     explanation.to_string(),
///     "zlib-1.3 is installed for so:libz.so.1, required by app-1.0\n\
///      app-1.0 is installed for app, required by the request"
/// );
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Explanation {
    steps: Vec<Step>,
}

impl Explanation {
    /// The steps, each before every step it follows from.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Whether there is no step: a refusal of a requirement the request or
    /// the world stated explains nothing further.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

impl fmt::Display for Explanation {
    /// Writes each step on a line of its own.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (at, step) in self.steps.iter().enumerate() {
            if at > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{step}")?;
        }
        Ok(())
    }
}

/// Writes `packages` separated by commas.
fn write_list(f: &mut fmt::Formatter<'_>, packages: &[Package]) -> fmt::Result {
    for (at, package) in packages.iter().enumerate() {
        if at > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{package}")?;
    }
    Ok(())
}

/// Steps, each with the steps it follows from: what explanations are read
/// out of.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Graph {
    steps: Vec<Step>,
    /// For each step, the steps it follows from, in the order its reason
    /// names them.
    causes: Vec<Vec<usize>>,
}

impl Graph {
    /// Adds `step`, following from nothing yet, and returns its place.
    pub(super) fn push(&mut self, step: Step) -> usize {
        self.steps.push(step);
        self.causes.push(Vec::new());
        self.steps.len().saturating_sub(1)
    }

    /// Notes that the step at `from` follows from the one at `cause`.
    pub(super) fn link(&mut self, from: usize, cause: usize) {
        if let Some(causes) = self.causes.get_mut(from)
            && !causes.contains(&cause)
        {
            causes.push(cause);
        }
    }

    /// The step at `at`.
    pub(super) fn step(&self, at: usize) -> Option<&Step> {
        self.steps.get(at)
    }

    /// The explanation of the steps at `roots`: each, and every step it
    /// follows from, each step before those it follows from.
    ///
    /// The order is the reverse of the order a depth-first walk from the
    /// roots finishes the steps in, which puts each before its causes
    /// wherever the steps form no cycle; of steps neither follows from, the
    /// first root's come first.
    pub(super) fn explain(&self, roots: &[usize]) -> Explanation {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut finished: Vec<usize> = Vec::new();
        for &root in roots.iter().rev() {
            if !seen.insert(root) {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
            while let Some((at, next)) = stack.last_mut() {
                let at = *at;
                let cause = self
                    .causes
                    .get(at)
                    .and_then(|causes| causes.get(*next))
                    .copied();
                *next = next.saturating_add(1);
                match cause {
                    Some(cause) => {
                        if seen.insert(cause) {
                            stack.push((cause, 0));
                        }
                    }
                    None => {
                        stack.pop();
                        finished.push(at);
                    }
                }
            }
        }
        Explanation {
            steps: finished
                .into_iter()
                .rev()
                .filter_map(|at| self.steps.get(at).cloned())
                .collect(),
        }
    }
}
