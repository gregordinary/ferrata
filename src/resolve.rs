//! Resolution: the packages `apk add` installs for a request.
//!
//! A [`Resolver`] holds the package records of the repositories a root is
//! configured with, in the order it is configured with them, and the
//! packages installed in the root and its world, if it has any. It answers
//! [`Request`]s over them: each answer is a [`Resolution`], the packages the
//! root holds afterwards and the [`Change`]s apk would make to it, in the
//! order it would make them, or a [`Refusal`], which names the requirement
//! apk could not meet. Either one explains itself: a resolution says why
//! each package is in it and why each package it drops goes, and a refusal
//! how the requirement it could not meet came about (see [`Explanation`]).
//! A resolution also says where each of its packages comes from
//! ([`Resolution::source`]): the root, or the repository that offered it;
//! each change that installs a package names that repository too.
//! [`order`] orders any set of packages installed together, as apk installs
//! them.
//!
//! ```
//! use ferrata::Dialect;
//! use ferrata::index;
//! use ferrata::resolve::{Request, Resolver};
//!
//! let text = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:app\nV:1.0-r0\nD:so:libz.so.1\n\n\
//!             C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:zlib\nV:1.3-r0\np:so:libz.so.1=1.3\n";
//! let records: Vec<_> = index::read(Dialect::V3_0, text)
//!     .map(|located| located.map(index::Located::into_record))
//!     .collect::<Result<_, _>>()?;
//! let resolver = Resolver::new(Dialect::V3_0).repository(&records);
//! let request: Request = [Dialect::V3_0.dependency("app")?].into_iter().collect();
//! let resolution = resolver.resolve(&request)?;
//! let names: Vec<_> = resolution.packages().iter().map(|record| record.name()).collect();
//! assert_eq!(names, ["zlib", "app"]);
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```
//!
//! # How apk resolves
//!
//! These are apk's rules as measured; each names the scenario of the
//! behavior catalog that shows it.
//!
//! - **Candidates.** A package is a candidate for a name it carries, at its
//!   own version, and for each name it provides, at the version it provides
//!   it at or at none. Every version a repository offers is a candidate, not
//!   only the latest (`versions/an-older-version-answers-a-constraint`). A
//!   name provided at no version stands below every version: it answers a
//!   dependency that states no version, and one whose operator admits an
//!   earlier version
//!   (`providers/a-provider-at-no-version-answers-an-entry-admitting-earlier-versions`,
//!   `providers/a-dependency-admitting-earlier-versions-is-met-at-no-version`),
//!   in a condition and a `!` entry too
//!   (`install-if/a-condition-admitting-earlier-versions-is-met-at-no-version`,
//!   `conflicts/a-conflict-admitting-earlier-versions-forbids-a-provider-at-no-version`).
//!   [`Record::answers`] asks this of one record and an entry, and
//!   [`Requirement::answered_by`] of one record and a requirement, by the
//!   same rule.
//! - **Choosing.** Of the candidates every requirement on a name admits, apk
//!   installs the one providing it at a version over one providing it at
//!   none, then the later version, then the higher priority (`k:`), then the
//!   record listed first, the repositories in the order configured
//!   (`providers/`, `repositories/`). A provider at no version is chosen only
//!   with a priority above 0. A package carrying the name has no precedence
//!   over a provider of it. Two versions of one package that are candidates
//!   at one version, or both at none, go by their own version before their
//!   priorities (`providers/of-two-versions-providing-a-name-alike-the-later-is-chosen`,
//!   `providers/of-two-versions-providing-a-name-at-no-version-the-later-is-chosen`);
//!   apk takes the candidates in turn, keeping the better of each pair,
//!   which with this is not an order
//!   (`providers/of-two-versions-providing-a-name-alike-the-later-is-chosen-in-turn`).
//! - **One package to a held name.** A name a package carries, or provides
//!   at a version, is held by that package alone: no other package
//!   providing it, at a version or at none, is installed with it
//!   (`providers/two-providers-of-a-name-at-a-version-conflict`,
//!   `providers/a-holder-excludes-a-provider-at-no-version`). Names provided
//!   only at no version are shared
//!   (`providers/a-provider-at-no-version-shares-the-name`).
//! - **Requirements.** What an installed package depends on is required,
//!   with the constraint it states, and so is every name the request names.
//!   A requirement binds every package providing its name, one providing it
//!   at no version included
//!   (`providers/a-requirement-binds-a-provider-at-no-version`). A name every
//!   candidate for a required name depends on is required too, without a
//!   constraint (`decision-order/a-dependency-every-candidate-shares-is-taken-first`,
//!   `decision-order/a-shared-dependency-carries-no-constraint`); where the
//!   required name has a single candidate, with the constraint of that
//!   candidate's first entry on the name
//!   (`decision-order/a-sole-candidate-s-dependency-is-required-with-its-constraint`,
//!   `decision-order/a-sole-candidate-s-first-entry-on-a-name-is-the-one-required`),
//!   which moves a package installed already that the constraint does not
//!   admit (`installed/a-sole-candidate-s-constraint-upgrades-an-installed-package`);
//!   so does a candidate apk would choose once the others are ruled out
//!   (`decision-order/a-candidate-left-alone-requires-its-dependencies-with-their-constraints`,
//!   `reduced/libgedit-tepl-dev-upgrades-an-installed-libgedit-gfls`).
//!   apk collects requirements before it decides, so a constraint met late
//!   in the walk still narrows the choice
//!   (`decision-order/a-constraint-met-later-narrows-the-choice-a-first`).
//! - **Decision order.** apk decides each required name once, and never
//!   revisits a decision. It decides a name some package carries before a
//!   name packages only provide, whatever order they are reached in
//!   (`decision-order/a-real-name-before-a-virtual-one-virtual-first`). Of
//!   the names some package carries, it decides the one the walk below
//!   numbers last first
//!   (`decision-order/carried-names-are-decided-the-last-numbered-first`,
//!   `decision-order/carried-names-are-decided-the-last-numbered-first-not-the-last-required`),
//!   and one only after the undecided names that require it: one some
//!   package carries, and one provided only, whose single candidate depends
//!   on it, or on a name one of its candidates provides, and may be chosen
//!   for it (`decision-order/a-requirer-is-decided-before-what-it-requires-z-m`,
//!   `decision-order/a-sole-candidate-with-a-real-dependency-is-installed-first`,
//!   `decision-order/a-requirer-reaches-through-the-names-a-candidate-provides`,
//!   `decision-order/a-carried-name-through-a-provided-name-is-not-a-requirer`,
//!   `decision-order/a-requirer-whose-candidate-cannot-be-chosen-waits`);
//!   and after one that would install one of its candidates: a name a
//!   candidate carries, and what would require that name
//!   (`decision-order/a-name-a-candidate-carries-is-decided-first`). A
//!   provided name is decided there only if no other name could still
//!   require it: no package depending on it is a candidate for a name
//!   pending or reached by the walk for the preliminary choices, but a
//!   pending carried name
//!   (`decision-order/a-requirer-another-pending-name-could-require-waits`,
//!   `decision-order/a-requirer-a-package-the-walk-reaches-depends-on-waits`,
//!   `decision-order/a-requirer-a-pending-carried-name-could-require-is-decided-first`);
//!   the names that would require it are decided before it in turn, in the
//!   same way (`decision-order/a-requirer-s-requirers-are-decided-first`,
//!   `reduced/kmail-takes-kwallet-alone`).
//!   Names packages only provide are decided in the order a depth-first walk
//!   from the request reaches them, through every candidate, whether chosen
//!   or not (`decision-order/provided-names-are-decided-depth-first-v-reached-first`,
//!   `decision-order/the-walk-passes-through-candidates-not-chosen`). From
//!   each name it reaches, and each candidate's own name, the walk goes on
//!   to the packages whose condition names it
//!   (`install-if/a-condition-naming-a-candidate-has-its-turn-there`), but
//!   only to those whose condition the preliminary choices below complete:
//!   not to one whose condition could hold but names a package nothing
//!   requires (`install-if/a-condition-the-choices-do-not-complete-numbers-nothing`),
//!   a candidate not chosen
//!   (`install-if/a-condition-naming-a-candidate-not-chosen-numbers-nothing`)
//!   or a version not chosen
//!   (`install-if/a-condition-naming-a-version-not-chosen-numbers-nothing`),
//!   nor to one whose condition can never hold
//!   (`install-if/a-condition-that-never-holds-numbers-nothing`). It does
//!   not follow the names a condition states
//!   (`install-if/the-names-a-condition-states-number-nothing`). A name
//!   packages only provide, none of whose candidates apk would choose for
//!   it, waits until no other name is left to decide, by which time a
//!   package chosen for another may answer it
//!   (`decision-order/a-name-no-candidate-apk-would-choose-answers-is-decided-last`,
//!   `decision-order/a-name-no-candidate-apk-would-choose-answers-waits-for-a-dependency-of-its-candidate`);
//!   a name some package carries does not wait
//!   (`decision-order/a-name-some-package-carries-is-decided-though-no-candidate-apk-would-choose-is-left`).
//!   A pending name every candidate of which holds the name being decided
//!   narrows that decision to those candidates, ranked as its own
//!   (`decision-order/a-name-every-candidate-of-which-holds-another-narrows-its-choice`,
//!   `decision-order/a-name-narrows-another-to-its-candidates-by-the-other-s-ranking`);
//!   not where one candidate does not hold it
//!   (`decision-order/a-name-a-candidate-of-which-does-not-hold-another-narrows-nothing`),
//!   nor where two such names narrow it differently
//!   (`decision-order/two-names-narrowing-a-name-differently-narrow-nothing`),
//!   nor where only packages installed for their condition require the name
//!   (`install-if/a-name-only-a-package-installed-for-its-condition-requires-narrows-nothing`).
//! - **The request.** The request is a set, walked in name order, which
//!   ignores ASCII case
//!   (`request/requested-names-are-decided-in-name-order-ignoring-case`); a
//!   requested name with a single candidate is installed before the others
//!   are decided (`request/`), after the names that would require it
//!   (`request/a-requested-name-with-one-candidate-is-settled-after-its-requirers`).
//!   A `!` entry in it rules out what it names
//!   before anything is decided
//!   (`conflicts/a-conflict-in-the-request-rules-out-what-it-names`).
//! - **Ruling out.** A candidate is passed over once it cannot be
//!   installed: a requirement on a name it provides does not admit it, a name
//!   it provides is held by another package, it would hold a name another
//!   provides already, a `!` entry of a package installed forbids it, a name
//!   it depends on is held by a package its dependency does not admit, or a
//!   dependency of it could only be met by packages ruled out already
//!   (`decision-order/a-candidate-needing-only-ruled-out-packages-is-passed-over`).
//!   A required name left with a single candidate holds what that candidate
//!   would hold before it is decided, and rules out what that excludes
//!   (`decision-order/a-sole-candidate-holds-its-names-before-it-is-chosen`).
//! - **No lookahead of its own.** Nothing else rules a candidate out. One
//!   whose dependency no package could ever meet is chosen, and the request
//!   refused (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over`),
//!   even where a package installed holds the name it needs
//!   (`refusals/a-holder-does-not-rule-out-a-dependent-nothing-could-meet`);
//!   so is one whose `!` entry forbids a package installed already
//!   (`conflicts/a-candidate-forbidding-an-installed-package-is-not-passed-over`),
//!   and one that conflicts with itself
//!   (`providers/a-package-conflicting-with-itself-is-still-chosen`,
//!   `install-if/a-package-conflicting-with-itself-refuses-once-its-condition-holds`).
//!   One exception: where a candidate for a name depends on a package up
//!   the chain of requirements that brought the name, or on one installed
//!   for the request or the world, the candidates whose dependency nothing
//!   installed or open admits are passed over
//!   (`refusals/a-candidate-depending-back-on-its-requirer-is-passed-over-when-unmet`,
//!   `refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-depending-on-its-requirer`,
//!   `refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-depending-further-up`),
//!   but not for a name the request or the world states itself
//!   (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over-for-a-requested-name`).
//!   The chain runs through the name each package on it was chosen for, a
//!   name it only provides included
//!   (`refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-depending-further-up-through-a-provided-name`),
//!   and ends at a package installed for its condition
//!   (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over-beside-one-depending-above-a-condition`).
//! - **Conflicts.** A package's `!` dependency forbids every package the
//!   entry admits, a package providing the name included
//!   (`conflicts/a-conflict-reaches-a-provided-name`,
//!   `conflicts/a-conflict-reaches-a-provider-at-no-version`), but not the
//!   package stating it (`conflicts/a-package-may-conflict-with-a-name-it-provides`).
//! - **`install_if`.** A package is installed as soon as its condition holds:
//!   every package the condition names installed, at a version the entry
//!   admits, and none it names with `!`
//!   (`install-if/installed-as-soon-as-its-condition-holds`). A condition
//!   whose `!` entry names a name the package itself holds, its own or one
//!   it provides at a version, never holds, whatever version the entry
//!   admits (`install-if/a-condition-forbidding-the-package-s-own-name-never-holds`,
//!   `install-if/a-condition-forbidding-a-name-the-package-provides-at-a-version-never-holds`).
//!   A condition of `!` entries alone holds only once a package of a name
//!   it names is installed, at a version its entry does not forbid
//!   (`install-if/a-condition-of-negated-entries-alone-needs-a-package-it-names`,
//!   `install-if/a-condition-of-negated-entries-alone-needs-a-package-of-a-name-it-names-installed`).
//!   Where the walk
//!   reaches it, the package has a turn of its own in the order provided
//!   names are decided, and its dependencies are decided from there
//!   (`install-if/a-dependency-is-decided-where-the-walk-reaches-the-package`);
//!   a dependency with a single candidate is installed with it at once
//!   (`install-if/a-dependency-with-one-candidate-is-installed-at-once`). A
//!   `!` entry is judged only once nothing left to decide could install what
//!   it names (`install-if/a-negated-entry-waits-for-the-package-it-names`),
//!   the package itself included where it provides the name at no version
//!   (`install-if/a-condition-forbidding-a-name-the-package-provides-at-no-version-waits-for-the-package-itself`).
//!   A package whose condition holds and whose dependency cannot be met, or
//!   whose `!` entry forbids a package installed, refuses the request
//!   (`install-if/an-unmet-dependency-refuses-the-request`,
//!   `install-if/a-conflict-with-the-request-refuses-it`), unless it
//!   depends on a name a package completing its condition provides, or
//!   forbids that package: then it is passed over
//!   (`install-if/an-unmet-dependency-passes-over-a-package-needing-its-condition`,
//!   `install-if/an-unmet-dependency-passes-over-a-package-needing-a-name-its-condition-s-package-provides`,
//!   `install-if/a-dependency-the-condition-s-package-cannot-meet-passes-over-the-package`,
//!   `install-if/a-package-forbidding-what-completes-its-condition-is-passed-over`,
//!   `install-if/a-conflict-passes-over-a-package-needing-its-condition`),
//!   though not where it needs that package only through another
//!   (`install-if/an-unmet-dependency-refuses-where-the-package-needs-its-condition-only-through-another`).
//!   One ruled out already is passed over
//!   (`install-if/one-whose-dependency-is-ruled-out-is-passed-over`). Below
//!   a package installed for its condition, a package holding a name a `!`
//!   entry of that condition names, whatever version the entry admits, is
//!   no candidate: the package's own dependency left with none passes it
//!   over (`install-if/a-condition-s-negated-entry-keeps-its-name-out-of-the-package-s-dependencies`),
//!   another candidate is taken
//!   (`install-if/a-condition-s-negated-entry-leaves-another-candidate`),
//!   and a dependency further down left with none is refused
//!   (`install-if/a-condition-s-negated-entry-refuses-a-dependency-further-down`).
//!   A name required from outside the package is unaffected
//!   (`install-if/a-condition-s-negated-entry-leaves-a-name-required-from-outside`).
//!
//! apk also lets the best candidates of names it has yet to decide steer a
//! choice it makes now. Before deciding anything it makes a preliminary
//! choice for each name and for each package with a condition, in the order
//! a second walk reaches them, one that also follows the names a condition
//! states (`decision-order/a-name-reached-through-a-condition-is-chosen-for-there`)
//! and takes the packages whose condition names a name in the order of
//! their names (`install-if/conditions-a-name-completes-are-taken-in-name-order`).
//! A name takes the package chosen already that answers it
//! (`decision-order/a-name-a-choice-answers-takes-that-package`), though
//! that steers no name some package carries that the request or the world
//! names
//! (`decision-order/a-requested-name-some-package-carries-keeps-its-best-candidate-against-an-earlier-choice`), or its
//! best candidate that fits beside the choices made; a package with a
//! condition is chosen once the choices complete its condition
//! (`install-if/a-condition-that-will-hold-steers-an-earlier-choice`). A
//! later choice overrides an earlier one it cannot stand beside: one it
//! forbids (`conflicts/a-conflict-of-a-later-name-steers-an-earlier-choice`),
//! or one every candidate of a dependency of it clashes with
//! (`decision-order/a-later-name-whose-best-candidate-needs-a-candidate-steers-an-earlier-one`),
//! but not where that dependency's one candidate is a provider at no version
//! without a priority
//! (`decision-order/a-dependency-only-a-provider-apk-would-not-choose-answers-steers-nothing`).
//! A name some package carries that the request or the world names keeps a
//! choice its entry admits against a package with a condition and against a
//! name packages only provide
//! (`decision-order/a-requested-name-some-package-carries-keeps-its-choice-against-a-provided-name`,
//! `install-if/a-requested-name-some-package-carries-keeps-its-choice-against-a-turn`);
//! a carried name's choice still overrides it
//! (`decision-order/a-requested-name-some-package-carries-follows-a-carried-name`),
//! and so does any choice where its entry does not admit it
//! (`decision-order/a-requested-name-some-package-carries-is-steered-from-a-package-it-does-not-admit`).
//! A name the request does not name whose best candidate forbids the choice
//! for a name the request or the world names, which that name's entry
//! admits, takes its next candidate instead, where it has one
//! (`conflicts/a-later-name-gives-way-to-a-requested-name-its-best-candidate-forbids`,
//! `installed/a-world-name-gives-way-to-a-requested-name`); a name the
//! request names does not
//! (`conflicts/a-conflict-between-two-requested-names-steers-neither`).
//! A choice kept from its name's best candidate by an earlier one reserves
//! that candidate's names against it
//! (`install-if/a-condition-that-never-holds-reserves-a-provider`), unless a
//! package installed for a carried name answers the earlier one
//! (`decision-order/a-choice-a-carried-package-answers-reserves-nothing`).
//! Only the names the request reaches through the choices count
//! (`decision-order/a-name-nothing-requires-reserves-nothing`). A decision
//! takes its name's choice where the choice was steered so and is still a
//! candidate.
//!
//! Over a whole repository, the walk for the preliminary choices reaches
//! much of it through conditions, but what it reaches through a package
//! that will not be installed gives nothing a place in the decision order
//! (`install-if/a-condition-reached-through-a-condition-numbers-nothing`).
//!
//! # Over an installed set
//!
//! A root's world is the request that made it what it is. apk resolves a
//! request over a root as it resolves the world with the request's entries
//! added, the entry of the request replacing the world's for a name both
//! name; every rule above holds, with these, each measured.
//!
//! - **Kept.** A package installed is a candidate as any other, and keeps
//!   its name where nothing the request needs stands in the way: against a
//!   later version (`installed/an-installed-package-is-kept-though-a-later-version-is-offered`),
//!   another build of its version
//!   (`installed/another-build-of-the-installed-version-is-not-taken`), and
//!   a provider at a version or a higher priority
//!   (`installed/an-installed-provider-outranks-a-higher-priority-at-a-version`,
//!   `installed/an-installed-provider-at-no-version-outranks-a-provider-at-a-version`),
//!   though no repository offers it any more
//!   (`installed/an-installed-package-no-repository-offers-is-kept`). Of two
//!   providers at no version, the higher priority goes first, installed or
//!   not (`installed/a-higher-priority-outranks-an-installed-provider-at-no-version`).
//!   A package installed is listed before the repositories' among a name's
//!   candidates, and apk takes them in turn, keeping the better of each
//!   pair; with a package installed, that is not an order, and the order
//!   the repositories list the others in can decide
//!   (`installed/an-installed-provider-is-compared-first-bb-cc`).
//! - **Changed where needed.** A package installed is replaced where a
//!   requirement does not admit it: upgraded, downgraded
//!   (`installed/a-downgrade-the-request-needs`), or replaced by another
//!   build of its version that the request names
//!   (`installed/another-build-requested-by-its-identity-replaces-the-installed-one`);
//!   and so is one standing in the way of what the request needs
//!   (`installed/an-upgrade-the-request-needs-reaches-an-installed-dependent`),
//!   or of what an installed package needs
//!   (`installed/an-installed-dependency-that-no-longer-meets-a-constraint-is-replaced`).
//!   One that cannot be replaced refuses the request
//!   (`installed/an-installed-package-that-cannot-change-refuses-the-request`).
//! - **Removed.** A package installed that the world and the request no
//!   longer reach is removed
//!   (`installed/a-package-the-world-no-longer-reaches-is-removed`), and so
//!   is one whose `install_if` condition no longer holds
//!   (`installed/a-package-whose-condition-no-longer-holds-is-removed`).
//! - **The world binds.** A world entry's constraint, or the identity it
//!   names, holds as a request entry's does
//!   (`installed/a-version-pinned-in-the-world-holds`,
//!   `installed/a-record-pinned-in-the-world-holds`), and a world apk cannot
//!   resolve refuses any request
//!   (`installed/a-world-entry-nothing-provides-refuses-the-request`,
//!   `installed/an-installed-package-s-unmet-dependency-refuses-the-request`).
//! - **The walks.** Names are decided in name order, the world's and the
//!   request's together
//!   (`installed/world-and-request-names-are-decided-in-name-order`); the
//!   preliminary choices take the request's names before the rest of the
//!   world's (`installed/a-choice-for-the-request-steers-a-world-name`). apk
//!   reads the world's names, then the installed database's, before the
//!   repositories'
//!   (`installed/the-world-s-names-are-read-before-the-installed-database-s`,
//!   `installed/the-installed-database-s-names-are-read-before-the-repositories`),
//!   and orders removals by that order as it orders installs
//!   (`installed/the-world-s-names-are-read-first-for-the-order-of-removals`).
//! - **The changes.** A package of the resolution not installed already
//!   replaces the installed package of its name, or is installed. Removals
//!   come first, in the order of the installed database
//!   (`installed/removals-come-first`,
//!   `installed/removals-in-the-order-installed`); the rest follow install
//!   order, walked from the world and the request
//!   (`installed/changes-in-the-order-of-the-walk-from-the-world`). A
//!   removal waits for the packages needing it to be removed or replaced
//!   (`installed/removals-wait-for-what-needs-them`,
//!   `installed/a-removal-waits-for-every-package-that-needed-it`). At its
//!   place in the database it has those needing it through a name it
//!   provides at no version, then those needing it through a condition,
//!   removed first, each in the order of apk's table of names (see "How
//!   apk orders"), at the first of the names it carries and provides
//!   (`installed/a-removal-waiting-on-removals-pulls-them-first`,
//!   `installed/removals-a-removal-pulls-go-in-the-order-of-apk-s-table-of-names`,
//!   `installed/a-package-installed-for-its-condition-goes-after-a-package-needing-its-condition`,
//!   `installed/a-provider-at-no-version-goes-after-the-first-removals`).
//!   One needing it by the name it carries is followed by it at once, or
//!   for a package replaced, once the walk has placed what placing it led
//!   to (`installed/a-package-replaced-frees-what-it-needed`,
//!   `installed/a-package-replaced-frees-what-it-needed-after-what-it-triggers`);
//!   the packages one change frees go in the reverse of the order a
//!   depth-first walk through their needs leaves them, a dependency's
//!   needs taken last stated first, so in the order stated, each after what
//!   needed it (`installed/freed-packages-are-removed-in-the-order-stated`,
//!   `installed/freed-packages-are-removed-depth-first`,
//!   `installed/a-package-two-removals-free-goes-after-the-first-one-s-other-needs`),
//!   and a condition's in the reverse order. One needed through a name it
//!   provides at a version goes at its place only where nothing waits on it
//!   by then (`installed/a-package-freed-by-removals-goes-with-them-when-listed-after-them`),
//!   and otherwise last, as do removals that wait on each other
//!   (`installed/a-package-needed-through-a-name-it-provides-at-a-version-is-removed-last`,
//!   `installed/removals-in-a-cycle-come-last`): in the order of the
//!   database, each after the removals waiting on it, taken in the order of
//!   the table of names
//!   (`installed/the-last-removals-go-in-the-order-of-apk-s-table-of-names`),
//!   and followed by those it frees, the one the database lists last first
//!   (`installed/the-last-removals-a-removal-frees-go-the-one-given-last-first`).
//!   Where a package states two entries an installed package answers, the
//!   last says how it is needed
//!   (`installed/a-package-needed-by-name-and-through-a-provided-name-is-removed-last`,
//!   `installed/a-package-needed-last-by-name-follows`). A package replaced
//!   holds up no removal of a provider at no version, nor of one whose name
//!   another provides now
//!   (`installed/a-provider-at-no-version-is-removed-before-a-replacement-of-what-needed-it`,
//!   `installed/a-provider-replaced-for-a-package-replaced-is-removed-first`).
//!   A removal whose package provides a name a package the resolution
//!   brings provides too, in any form, goes, at the latest, just before the
//!   walk that places that package
//!   (`installed/a-removal-left-for-last-goes-before-a-package-providing-a-name-it-provides`),
//!   after the removals still waiting on it
//!   (`installed/a-displaced-package-goes-before-its-successor-after-the-removals-waiting-on-it`),
//!   though a package
//!   replaced still needed it; it is held back there, though nothing else
//!   holds it up, only where the name is its own or one a package kept or
//!   replaced needs
//!   (`installed/a-displaced-package-goes-before-its-successor-s-walk`,
//!   `installed/a-package-losing-a-name-nothing-else-needs-is-removed-first`,
//!   `installed/a-displaced-package-goes-before-its-successor-though-a-package-replaced-needed-it`,
//!   `installed/a-package-losing-a-name-it-provides-is-removed-at-the-turn-of-its-successor`,
//!   `installed/a-package-whose-own-name-another-takes-is-removed-at-its-turn`,
//!   `installed/a-package-a-provider-at-no-version-displaces-is-removed-at-its-turn`),
//!   though it still follows a removal that frees it
//!   (`installed/a-displaced-package-still-follows-a-removal-that-frees-it`).
//!   Where the walk follows a `!` entry, of the request, the world or a
//!   package it walks, a removal still to come carrying or providing its
//!   name, at whatever version, goes there, after the removals waiting on
//!   it, though a package replaced later still needed it
//!   (`installed/a-removal-a-conflict-entry-leads-the-walk-to-goes-there`,
//!   `installed/a-removal-a-world-conflict-leads-the-walk-to-goes-there`).
//!
//! # Explanations
//!
//! A [`Resolution`] notes why each package is in it, as a [`Reason`]: the
//! requirement it was chosen for, which the request, the world or a package
//! installed before it stated, or its `install_if` condition, which packages
//! installed before it meet. A package the root held and keeps has its
//! reason as any other. A package the root held and the resolution drops
//! has an [`Exclusion`]: what ruled it out, or that nothing chose it.
//! [`Resolution::explain`] follows these back to the request and the world,
//! and a [`Refusal`] carries the same chain for the requirement it could
//! not meet. The reasons are the solver's own, so where ferrata's answer
//! differs from apk's, its explanation shows the decisions that led there.
//! What is refused is what apk refuses; the explanations, and their
//! wording, are ferrata's.
//!
//! # How apk orders
//!
//! The order depends on the packages installed, the request, the records'
//! stated lists, and the names the repositories mention, and not on the
//! order apk decided names in, but where several packages installed answer
//! one dependency.
//!
//! - **The walk.** apk walks the request's entries in the order of their
//!   names, whichever packages answer them
//!   (`install-order/the-request-is-walked-in-the-order-of-its-entries-names`),
//!   and from each package its dependencies, in the order stated, not the
//!   order it decided them in
//!   (`install-order/dependencies-in-the-order-stated-not-the-order-decided`).
//!   A package is placed after what its dependencies lead to
//!   (`install-order/each-package-after-what-it-depends-on`), where the walk
//!   first reaches it
//!   (`install-order/a-shared-dependency-goes-where-it-is-first-reached`).
//! - **Cycles.** A package the walk reaches while walking it is passed over
//!   there, so a cycle is broken where it closes
//!   (`install-order/a-cycle-is-broken-where-it-closes-from-a`).
//! - **`!` entries.** A `!` entry leads the walk too, at its place among
//!   the dependencies or the request, to the package carrying or providing
//!   its name at whatever version
//!   (`install-order/a-conflict-entry-orders-what-it-names`,
//!   `install-order/a-conflict-in-the-request-orders-what-it-names`).
//! - **`install_if`.** Placing a package triggers each package whose
//!   condition names it: that package is walked next, its dependencies
//!   before it, even inside another package's walk
//!   (`install-order/install-if-inside-the-walk`,
//!   `install-order/install-if-with-its-own-dependencies`), once every
//!   package its condition names but by a `!` entry has been reached,
//!   placed or still being walked, and none it names by a `!` entry
//!   (`install-order/install-if-placed-while-its-condition-is-being-walked`,
//!   `install-order/a-package-whose-condition-is-not-reached-waits`,
//!   `install-order/a-condition-a-package-reached-forbids-is-not-walked-there`). A
//!   `!` entry of a condition triggers its package too, when the package
//!   placed is one it does not forbid
//!   (`install-order/install-if-a-negated-entry-the-package-placed-escapes-triggers`,
//!   `install-order/install-if-a-negated-entry-forbidding-the-package-placed-does-not-trigger`).
//!   One that a package depends on is placed where the walk reaches it,
//!   condition or not
//!   (`install-order/install-if-reached-as-a-dependency-is-placed-there`).
//!   A package not installed whose condition names the package placed leads
//!   the walk too, to the packages installed carrying or providing a name
//!   it carries or provides
//!   (`install-order/a-package-not-installed-leads-through-a-name-it-provides`);
//!   one installed that the walk has not reached leads, through a name it
//!   provides, to a package its condition does not name
//!   (`install-order/a-package-whose-condition-may-hold-leads-to-a-provider-it-does-not-name`),
//!   and one being walked or placed leads nowhere
//!   (`install-order/a-package-being-walked-leads-nowhere-through-its-names`).
//! - **Several triggered.** Placing a package triggers packages name by
//!   name: its own name, then each name it provides, in the order stated
//!   (`install-order/each-name-of-the-package-placed-triggers-in-turn`).
//!   For each, the packages it triggers are walked in the order apk's table
//!   of names holds the names they carry and provide, each at the first of
//!   its names (`install-order/packages-one-placement-triggers-in-apk-s-table-order`,
//!   `install-order/a-triggered-package-is-walked-at-the-first-of-its-names`).
//!   The table has 20,000 buckets, visited in ascending order; a name's
//!   bucket is its 32-bit MurmurHash3 with seed 5381, modulo 20,000, and of
//!   names in one bucket, the one apk read last comes first
//!   (`install-order/names-sharing-a-bucket-the-one-read-last-first-xp-read-last`).
//!   apk reads names where the repositories first mention them, in the
//!   order it is given the repositories, each record's fields in the order
//!   they stand and each list's entries in the order stated
//!   (`install-order/a-name-is-read-where-the-index-first-mentions-it-xp-mentioned-first`,
//!   `install-order/a-record-s-names-are-read-in-the-order-its-fields-stand`,
//!   `install-order/names-of-a-later-repository-are-read-later`).
//! - **Several providers.** Where several packages installed answer one
//!   dependency, the walk follows the one apk installed first: one
//!   requested, the first by name
//!   (`install-order/two-requested-providers-the-first-by-name`), the one
//!   chosen for the name (`install-order/a-requested-provider-is-followed`),
//!   or of several each installed for another package, the one whose name
//!   apk decided first
//!   (`install-order/providers-installed-for-others-the-one-installed-first`).
//!
//! # Where ferrata answers otherwise
//!
//! A difference from apk on any input is a defect unless it is listed here.
//! These are the shapes measured where apk answers otherwise than ferrata
//! does, each with the catalog scenario that holds apk's answer. Each item
//! states what apk does and ferrata does not, unless it says otherwise.
//! Most are observed rather than understood: the scenario shows what apk
//! does, and the rule behind it is not yet known.
//!
//! - **Passing over a candidate whose dependency cannot be met.** apk
//!   passes over such a candidate beside one depending on a package the
//!   request names
//!   (`refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-depending-on-a-requested-package`),
//!   and beside one needing the name the candidate carries
//!   (`refusals/a-candidate-whose-dependency-cannot-be-met-is-passed-over-beside-one-needing-its-name`);
//!   ferrata refuses. Where a name has several requirers, apk does not pass
//!   over beside one depending on the second of them, and ferrata does
//!   (`refusals/a-candidate-whose-dependency-cannot-be-met-is-not-passed-over-beside-one-depending-on-the-second-of-two-requirers`).
//! - **Packages installed for their condition.** A package whose condition
//!   the walk completes waits for its turn where apk's table puts it after
//!   another package's decision
//!   (`install-if/a-condition-the-walk-reaches-waits-for-its-turn`). A
//!   package installed for its condition does not answer a name at no
//!   version required before it was installed
//!   (`install-if/a-package-installed-for-its-condition-does-not-answer-a-name-required-before`).
//!   A package whose condition holds waits for the decision of a name it is
//!   a candidate for
//!   (`install-if/a-package-whose-condition-holds-waits-for-the-decision-of-a-name-it-is-a-candidate-for`).
//!   A condition forbidding a package placed later still keeps its package
//!   from the walk until something needing it reaches it
//!   (`install-order/a-condition-forbidding-a-package-to-come-keeps-its-package-from-the-walk`).
//! - **Choices steered by names decided later.** apk follows these, and
//!   ferrata does not:
//!   - a name some package carries, left only a provider at no version,
//!     takes it once it is chosen for another name
//!     (`decision-order/a-carried-name-left-only-a-provider-at-no-version-takes-it-once-chosen`);
//!   - a candidate is passed over whose dependency only packages holding a
//!     name every candidate provides answer
//!     (`decision-order/a-candidate-whose-dependency-only-holders-of-a-name-every-candidate-provides-answer-is-passed-over`);
//!   - a provided name's single candidate stating two entries on one name
//!     has that name decided on the first entry before the candidate is
//!     installed
//!     (`decision-order/a-provided-name-s-only-candidate-s-first-entry-decides-a-name-before-it-is-installed`);
//!   - an earlier choice whose `!` entry forbids, through a name it
//!     provides, the one candidate of a later name gives way
//!     (`conflicts/a-choice-forbidding-a-later-name-s-one-candidate-through-a-provided-name-gives-way`);
//!   - where the preliminary choices do not settle, apk passes over a
//!     candidate needing what it forbids
//!     (`decision-order/choices-that-do-not-settle-pass-over-a-candidate-needing-what-it-forbids`),
//!     one its dependency forbids
//!     (`decision-order/choices-that-do-not-settle-pass-over-a-candidate-its-dependency-forbids`),
//!     and one whose dependency cannot be met
//!     (`decision-order/choices-that-do-not-settle-pass-over-a-candidate-whose-dependency-cannot-be-met`),
//!     as one of the alternating choices would.
//!
//!   And apk does not follow these, which ferrata does:
//!   - a carried name steered by a choice for a name packages only provide
//!     (`decision-order/a-carried-name-is-not-steered-by-a-provided-name-s-choice`);
//!   - a requested name packages only provide passed over for a choice it
//!     cannot stand beside
//!     (`decision-order/a-requested-provided-name-keeps-its-best-candidate-against-a-choice-it-cannot-stand-beside`);
//!   - a requested name steered by a conflict under another requested name
//!     (`conflicts/a-conflict-under-a-requested-name-does-not-steer-another-requested-name`);
//!   - a requested name taking the provider of a requested name packages
//!     only provide
//!     (`decision-order/a-requested-name-keeps-its-best-candidate-beside-a-requested-provided-name`);
//!   - a choice steered by a conflict reached through its condition
//!     (`decision-order/a-choice-a-conflict-reached-through-its-condition-forbids-is-kept`).
//! - **Over an installed set.** apk keeps an installed version, and
//!   refuses, where a chain of conditions names it and a replaced package
//!   needed it
//!   (`installed/a-chain-of-conditions-keeps-an-installed-version-a-replaced-package-needed`),
//!   as over a real root where an installed condition names a version
//!   exactly (`reduced/llvm-test-utils-is-refused-over-an-installed-python3-pyc`).
//!   It keeps an installed package, and upgrades what it needs, where a
//!   later build of it needs nothing (`reduced/freecad-keeps-an-installed-soqt`).
//!   It does not pass over a requested installed package's dependency for
//!   its condition, and refuses
//!   (`installed/a-requested-installed-package-s-dependency-is-not-passed-over-for-its-condition`).
//! - **Providers of a name at no version, in real repositories.** Reduced
//!   from Alpine and postmarketOS releases: the provider of
//!   `dbus:org.freedesktop.Secrets` apk installs, alone or beside another
//!   (`reduced/plasma-mobile-meta-takes-kwallet-alone`,
//!   `reduced/akonadi-calendar-takes-gnome-keyring-beside-kwallet`,
//!   `reduced/plasma-mobile-meta-over-postmarketos-takes-kwallet-alone`);
//!   a carried name taken where a provider would answer a later requirement
//!   too (`reduced/postmarketos-base-ui-gnome-is-refused-over-polkit`); and,
//!   in order only, a package needing a name two installed providers answer,
//!   installed after the one apk installed first
//!   (`reduced/lua-lapis-eswidget-installs-lua-date-after-lua5-1`).

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::fmt;

use crate::dependency::Dependency;
use crate::record::{Identity, Record};
use crate::version::Version;
use crate::{Dialect, Name};

mod change;
mod explain;
mod lookahead;
mod order;
mod solve;
mod table;
mod universe;

use explain::Graph;
pub use explain::{Exclusion, Explanation, Reason, Requirement, Step};
use universe::Universe;

/// The package records of a root's repositories, and of the packages
/// installed in it, which answer requests.
///
/// Built once, with each repository's records in turn and the root's
/// installed set if it has one, and asked any number of requests. A request
/// changes nothing the resolver holds, so each is answered as a resolver
/// built for it alone would answer it: apk reads the names a request
/// states after the repositories', where it reads a root's world before
/// them (`install-order/the-request-s-names-are-read-after-the-repositories`).
/// A caller with several requests over one root builds one resolver for
/// them all.
#[derive(Debug, Clone)]
pub struct Resolver<'a> {
    dialect: Dialect,
    universe: Universe<'a>,
    /// Every repository's records, in the order added.
    records: Vec<&'a Record>,
    /// Each repository, in the order added: where its records end in
    /// `records`, and where the packages the universe first took from it end
    /// among the universe's, which it numbers in the order it takes them.
    repositories: Vec<(usize, usize)>,
    /// The packages installed in the root, in the order given.
    installed: Vec<&'a Record>,
    /// The root's world, in the order given.
    world: Vec<&'a Dependency>,
}

impl<'a> Resolver<'a> {
    /// A resolver following `dialect`'s rules, for an empty root with no
    /// repositories.
    pub fn new(dialect: Dialect) -> Resolver<'a> {
        Resolver {
            dialect,
            universe: Universe::default(),
            records: Vec::new(),
            repositories: Vec::new(),
            installed: Vec::new(),
            world: Vec::new(),
        }
    }

    /// Adds a repository's records, after those of the repositories added
    /// before it.
    ///
    /// The order is the order a root is configured with its repositories in,
    /// and records keep the order the repository lists them in: of two
    /// candidates apk ranks equal, it installs the one listed first. The
    /// records are those of the root's architecture; a record apk cannot
    /// install (see [`Record::is_installable`]) is never chosen, but for one
    /// that conflicts with itself, which apk chooses as it would any other
    /// and then refuses the request.
    ///
    /// A record with the identity of one added before, in this repository,
    /// an earlier one, or the root's installed set, is that package offered
    /// again, and adds nothing, whatever its name: one of another name is
    /// no package of its own
    /// (`repositories/a-record-with-the-identity-of-another-name-is-no-package`,
    /// `repositories/a-record-with-the-identity-of-another-name-in-a-later-repository-is-no-package`,
    /// `installed/a-repository-record-with-the-identity-of-an-installed-package-is-no-package`),
    /// and one with the identity of a record apk cannot install is that
    /// record, which apk cannot install either
    /// (`repositories/a-record-with-the-identity-of-a-record-apk-cannot-install-is-that-record`,
    /// `installed/a-repository-record-with-the-identity-of-an-installed-record-apk-cannot-install-is-that-record`).
    ///
    /// Repositories are numbered in the order added, from 0, and a
    /// resolution names the one each package comes from by its number
    /// ([`Source::Repository`]); a call with no records is a repository
    /// all the same.
    #[must_use]
    pub fn repository(mut self, records: impl IntoIterator<Item = &'a Record>) -> Resolver<'a> {
        for record in records {
            self.universe.add(record);
            self.records.push(record);
        }
        self.repositories
            .push((self.records.len(), self.universe.package_count()));
        self
    }

    /// Makes the root one with packages installed: `records`, as its
    /// installed database lists them, and `world`, the entries of its world,
    /// as its world file lists them. It replaces any installed set given
    /// before, and may be given before or after the repositories: given
    /// before, the resolver is built once; given after, it is built again
    /// over every repository's records, since apk takes the root's names
    /// and packages before theirs.
    ///
    /// A request is then resolved together with the world, the request's
    /// entry replacing the world's for a name both name, and the answer is
    /// the changes apk makes to the root (see "Over an installed set" in
    /// the [module](self) documentation). A record installed that a
    /// repository offers too, or that is listed twice, its identity, is one
    /// package, as [`repository`](Resolver::repository) says. Of two
    /// installed records with one identity and two names, the one listed
    /// first is the package and the other none; apk 3.0.9 gives no answer
    /// for such a root.
    ///
    /// An installed record apk cannot install, as
    /// [`Record::is_installable`] says, is never kept. Where nothing needs
    /// it, it is removed
    /// (`installed/an-installed-package-apk-cannot-install-nothing-needs-is-removed`);
    /// a requirement on it is met by another package of its name
    /// (`installed/an-installed-package-apk-cannot-install-is-replaced`), or
    /// refuses the request
    /// (`installed/an-installed-package-apk-cannot-install-refuses-the-request`).
    ///
    /// The order of both lists counts: of two packages apk ranks equal, the
    /// one installed comes first, and apk reads the names of the world and
    /// of the installed records before any repository's, which decides the
    /// order of some packages it installs and removes (see "How apk
    /// orders").
    #[must_use]
    pub fn installed(
        mut self,
        records: impl IntoIterator<Item = &'a Record>,
        world: impl IntoIterator<Item = &'a Dependency>,
    ) -> Resolver<'a> {
        // A record with the identity of one listed before is that package
        // again.
        self.installed.clear();
        let mut listed = BTreeSet::new();
        for record in records {
            if listed.insert(record.identity()) {
                self.installed.push(record);
            }
        }
        self.world = world.into_iter().collect();
        // apk reads the world, then the installed database, then the
        // repositories, and lists the packages installed first among a
        // name's candidates (`installed/an-installed-provider-is-compared-first-bb-cc`):
        // the universe is built again in that order.
        let mut universe = Universe::default();
        universe.note_world(&self.world);
        for &record in &self.installed {
            universe.add_installed(record);
        }
        let mut start = 0;
        for (end, packages) in &mut self.repositories {
            for &record in self.records.get(start..*end).unwrap_or_default() {
                universe.add(record);
            }
            *packages = universe.package_count();
            start = *end;
        }
        self.universe = universe;
        self
    }

    /// The dialect whose rules the resolver follows.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Where `record`, a package of the resolver's, comes from: the root,
    /// where it held a package of its identity, or else the first
    /// repository that offered it, the one whose records the universe took
    /// the package from.
    fn source(&self, record: &Record) -> Source {
        let Some(package) = self.universe.package_of(record) else {
            // Every package of a resolution is one of the universe's.
            return Source::Installed;
        };
        if self.universe.held(package).is_some() {
            return Source::Installed;
        }
        self.repositories
            .iter()
            .position(|&(_, end)| package.index() < end)
            .map_or(Source::Installed, Source::Repository)
    }

    /// What `apk add` does to the root for `request`.
    ///
    /// # Errors
    ///
    /// Returns a [`Refusal`] where apk refuses the request, naming the
    /// requirement it could not meet.
    pub fn resolve(&self, request: &Request) -> Result<Resolution<'a>, Refusal> {
        let mut world: Request = self.world.iter().map(|&entry| entry.clone()).collect();
        world.extend(request.entries().cloned());
        let solved = solve::solve(&self.universe, &world, request)?;
        let placed = order::placements(
            solved.packages.iter().copied(),
            &world,
            Some(&self.universe),
        );
        let packages: Vec<&'a Record> = placed
            .records
            .iter()
            .filter_map(|&(index, _, _)| solved.packages.get(index).copied())
            .collect();
        let steps = placed
            .records
            .iter()
            .filter_map(|&(index, _, _)| solved.steps.get(index).copied());
        let spans: Vec<(usize, usize)> = placed
            .records
            .iter()
            .map(|&(_, end, start)| (start, end))
            .collect();
        let repositories: Vec<Option<usize>> = packages
            .iter()
            .map(|&record| match self.source(record) {
                Source::Repository(at) => Some(at),
                Source::Installed => None,
            })
            .collect();
        let changes = change::changes(
            &self.universe,
            &self.installed,
            &packages,
            &repositories,
            &spans,
            &placed.conflicts,
        );

        // Each package of the resolution, where it comes from and its step;
        // then each package the root held that the resolution drops, which
        // comes from the root: one apk cannot install is no candidate, and
        // the solver has no note of it.
        let mut graph = solved.graph;
        let mut known: BTreeMap<&Identity, (Source, Option<usize>)> = BTreeMap::new();
        for (&record, step) in packages.iter().zip(steps) {
            known
                .entry(record.identity())
                .or_insert((self.source(record), step));
        }
        let noted: BTreeMap<&Identity, usize> = solved
            .dropped
            .iter()
            .map(|&(record, at)| (record.identity(), at))
            .collect();
        for &record in &self.installed {
            let key = record.identity();
            if known.contains_key(&key) {
                continue;
            }
            let at = noted.get(&key).copied().unwrap_or_else(|| {
                graph.push(Step::Dropped {
                    package: Package::of(record),
                    exclusion: Exclusion::Uninstallable,
                })
            });
            known.insert(key, (Source::Installed, Some(at)));
        }
        Ok(Resolution {
            packages,
            changes,
            graph,
            known,
        })
    }
}

/// What is asked of `apk add`: dependency entries, at most one to a name.
///
/// apk reads a request as a set: the order of its entries plays no part,
/// and of two entries for one name, the later holds
/// (`request/the-later-of-two-entries-for-a-name-holds-install`). It goes
/// through the names in its own order: ignoring ASCII case, and byte by
/// byte where that ties (`request/requested-names-in-name-order-ignoring-case`).
///
/// ```
/// use ferrata::Dialect;
/// use ferrata::resolve::Request;
///
/// let mut request = Request::new();
/// request.push(Dialect::V3_0.dependency("musl")?);
/// request.push(Dialect::V3_0.dependency("busybox>=1.37")?);
/// request.push(Dialect::V3_0.dependency("musl>=1.2")?);
/// request.push(Dialect::V3_0.dependency("R")?);
/// let entries: Vec<String> = request.entries().map(|entry| entry.to_string()).collect();
/// assert_eq!(entries, ["busybox>=1.37", "musl>=1.2", "R"]);
/// # Ok::<(), ferrata::dependency::DependencyError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Request {
    entries: BTreeMap<Key, Dependency>,
}

/// A name, ordered as apk orders the names of a request: ignoring ASCII
/// case, then byte by byte, a byte that is not ASCII after every one that
/// is (`request/names-are-ordered-by-unsigned-bytes`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key(Name);

impl Ord for Key {
    fn cmp(&self, other: &Key) -> core::cmp::Ordering {
        fn folded(name: &Name) -> impl Iterator<Item = u8> + '_ {
            name.as_bytes().iter().map(u8::to_ascii_lowercase)
        }
        folded(&self.0)
            .cmp(folded(&other.0))
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Key) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Request {
    /// An empty request.
    pub fn new() -> Request {
        Request::default()
    }

    /// Adds `entry`, replacing any entry for its name added before.
    pub fn push(&mut self, entry: Dependency) {
        self.entries.insert(Key(entry.name().clone()), entry);
    }

    /// The entries, in the order apk goes through their names.
    pub fn entries(&self) -> impl Iterator<Item = &Dependency> {
        self.entries.values()
    }

    /// Whether the request holds no entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry for `name`, if there is one: text, or the bytes of a
    /// [`Name`].
    pub fn get(&self, name: impl AsRef<[u8]>) -> Option<&Dependency> {
        self.entries.get(&Key(Name::from(name.as_ref())))
    }
}

impl FromIterator<Dependency> for Request {
    fn from_iter<I: IntoIterator<Item = Dependency>>(entries: I) -> Request {
        let mut request = Request::new();
        request.extend(entries);
        request
    }
}

impl Extend<Dependency> for Request {
    fn extend<I: IntoIterator<Item = Dependency>>(&mut self, entries: I) {
        for entry in entries {
            self.push(entry);
        }
    }
}

/// What a request does to a root: the packages it holds afterwards, the
/// changes that take it there, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution<'a> {
    packages: Vec<&'a Record>,
    changes: Vec<Change<'a>>,
    graph: Graph,
    /// Each package of the resolution, and each package the root held that
    /// it drops, by its identity: where it comes from, and its step in
    /// `graph`.
    known: BTreeMap<&'a Identity, (Source, Option<usize>)>,
}

impl<'a> Resolution<'a> {
    /// Every package the root holds afterwards, those installed before
    /// included, in the order apk installs them (see [`order`]). For an
    /// empty root, the packages the request installs.
    pub fn packages(&self) -> &[&'a Record] {
        &self.packages
    }

    /// The changes apk makes to the root, in the order it makes them. For
    /// an empty root, each is an install, in the order of
    /// [`packages`](Resolution::packages).
    pub fn changes(&self) -> &[Change<'a>] {
        &self.changes
    }

    /// Why `record`, a package of the resolution, is in it: the
    /// requirement it was chosen for, or its `install_if` condition. A
    /// package the root held and keeps has its reason as any other.
    ///
    /// A record is a package of the resolution where its identity is that
    /// of one of [`packages`](Resolution::packages).
    pub fn reason(&self, record: &Record) -> Option<&Reason> {
        match self.step(record)? {
            Step::Installed { reason, .. } | Step::Kept { reason, .. } => Some(reason),
            Step::Required(_)
            | Step::Sole { .. }
            | Step::RuledOut { .. }
            | Step::Dropped { .. } => None,
        }
    }

    /// Why the resolution drops `record`, a package the root held: why it
    /// is removed, or replaced by another package of its name (an
    /// [`Upgrade`](Change::Upgrade), [`Downgrade`](Change::Downgrade) or
    /// [`Replace`](Change::Replace)).
    pub fn exclusion(&self, record: &Record) -> Option<&Exclusion> {
        match self.step(record)? {
            Step::Dropped { exclusion, .. } => Some(exclusion),
            Step::Installed { .. }
            | Step::Kept { .. }
            | Step::Required(_)
            | Step::Sole { .. }
            | Step::RuledOut { .. } => None,
        }
    }

    /// The explanation of `record`: of a package of the resolution, the
    /// chain of requirements that brought it in, back to the request or the
    /// world; of a package the root held and the resolution drops, what
    /// ruled it out and what that came from. None for any other record.
    ///
    /// For a change, the explanation of the package it installs says why
    /// the package is needed, and that of the package it replaces or
    /// removes, why that one goes.
    pub fn explain(&self, record: &Record) -> Option<Explanation> {
        Some(self.graph.explain(&[self.place(record)?]))
    }

    /// Where `record`, a package of the resolution or one the root held
    /// that it drops, comes from: the root, for a package the root held
    /// (by its identity), kept or dropped; otherwise the
    /// repository that offered it first. None for any other record.
    ///
    /// For a change, the source of the package it installs is where it is
    /// fetched from, which the change names as well, and the package it
    /// replaces or removes is the root's.
    ///
    /// ```
    /// use ferrata::{Dialect, index};
    /// use ferrata::resolve::{Change, Request, Resolver, Source};
    ///
    /// let read = |text: &str| -> Result<Vec<_>, index::ReadError> {
    ///     index::read(Dialect::V3_0, text)
    ///         .map(|located| located.map(index::Located::into_record))
    ///         .collect()
    /// };
    /// let main = read("C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:app\nV:1\nD:lib\n")?;
    /// let extra = read("C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:lib\nV:1\n")?;
    /// let resolver = Resolver::new(Dialect::V3_0).repository(&main).repository(&extra);
    /// let request: Request = [Dialect::V3_0.dependency("app")?].into_iter().collect();
    /// let resolution = resolver.resolve(&request)?;
    /// assert_eq!(resolution.source(&main[0]), Some(Source::Repository(0)));
    /// assert_eq!(resolution.source(&extra[0]), Some(Source::Repository(1)));
    /// // A change that installs a package names its repository as well.
    /// assert!(matches!(resolution.changes()[0], Change::Install { repository: 1, .. }));
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub fn source(&self, record: &Record) -> Option<Source> {
        self.known
            .get(&record.identity())
            .map(|&(source, _)| source)
    }

    /// The step about `record` in the graph.
    fn step(&self, record: &Record) -> Option<&Step> {
        self.graph.step(self.place(record)?)
    }

    /// Where the step about `record` is in the graph.
    fn place(&self, record: &Record) -> Option<usize> {
        self.known.get(&record.identity()).and_then(|&(_, at)| at)
    }
}

/// Where a package of a [`Resolution`] comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// The root, which held the package already.
    Installed,
    /// The repository at this position among those the [`Resolver`] was
    /// given, counting from 0: the first that offered the package.
    Repository(usize),
}

/// One change `apk add` makes to a root: one package installed, replaced,
/// or removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<'a> {
    /// A package installed where no package of its name was.
    Install {
        /// The package installed.
        package: &'a Record,
        /// The repository it comes from, counting from 0 as
        /// [`Source::Repository`] does.
        repository: usize,
    },
    /// The package of a name replaced by a later version.
    Upgrade {
        /// The package installed before.
        from: &'a Record,
        /// The package installed in its place.
        to: &'a Record,
        /// The repository `to` comes from, counting from 0 as
        /// [`Source::Repository`] does.
        repository: usize,
    },
    /// The package of a name replaced by an earlier version.
    Downgrade {
        /// The package installed before.
        from: &'a Record,
        /// The package installed in its place.
        to: &'a Record,
        /// The repository `to` comes from, counting from 0 as
        /// [`Source::Repository`] does.
        repository: usize,
    },
    /// The package of a name replaced by another package at the same
    /// version, one of another identity.
    Replace {
        /// The package installed before.
        from: &'a Record,
        /// The package installed in its place.
        to: &'a Record,
        /// The repository `to` comes from, counting from 0 as
        /// [`Source::Repository`] does.
        repository: usize,
    },
    /// A package installed before, removed.
    Remove(&'a Record),
}

/// The order `apk add` installs `records` in for `request`: each record's
/// index among them, every index once.
///
/// `records` is a set of packages installed together, such as a plan's
/// recorded one, and `request` the request that selected them; a set is
/// ordered without resolving it again. The rules are those under "How apk
/// orders" in the [module](self) documentation, but for three things a set
/// cannot say. Where several of its packages answer one dependency, the walk
/// follows the one given first, where apk follows the one it installed
/// first; a package outside the set whose condition names a package placed
/// leads apk's walk, but is not known here; and of names sharing a bucket of
/// apk's table, the one apk read last is judged by the set's records alone.
/// A [`Resolution`] orders its packages knowing all three, so its order is
/// the one to keep; ordering them again may differ from it.
///
/// A record that is a package given before (its identity) is placed with
/// it. The packages the walk from the request does not reach
/// are walked after the rest, in the order given, so each still follows
/// what its dependencies lead to.
///
/// ```
/// use ferrata::Dialect;
/// use ferrata::index;
/// use ferrata::resolve::{Request, order};
///
/// let text = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:app\nV:1\nD:zlib\n\n\
///             C:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:zlib\nV:1.3\n";
/// let records: Vec<_> = index::read(Dialect::V3_0, text)
///     .map(|located| located.map(index::Located::into_record))
///     .collect::<Result<_, _>>()?;
/// let request: Request = [Dialect::V3_0.dependency("app")?].into_iter().collect();
/// assert_eq!(order(&records, &request), [1, 0]);
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
pub fn order<'a>(records: impl IntoIterator<Item = &'a Record>, request: &Request) -> Vec<usize> {
    order::order(records, request, None)
}

/// apk's refusal of a request: the requirement it could not meet, and how
/// that requirement came about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(Box<Refused>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Refused {
    kind: RefusalKind,
    requirement: Requirement,
    holder: Option<Package>,
    explanation: Explanation,
}

impl Refusal {
    fn new(
        kind: RefusalKind,
        requirement: Requirement,
        holder: Option<Package>,
        explanation: Explanation,
    ) -> Refusal {
        Refusal(Box::new(Refused {
            kind,
            requirement,
            holder,
            explanation,
        }))
    }

    /// Why the requirement could not be met.
    pub fn kind(&self) -> RefusalKind {
        self.0.kind
    }

    /// The requirement that could not be met. For a
    /// [`RefusalKind::Forbidden`], the `!` entry, and what stated it.
    pub fn requirement(&self) -> &Requirement {
        &self.0.requirement
    }

    /// For a [`RefusalKind::Conflict`], the package installed in the
    /// requirement's way; for a [`RefusalKind::Forbidden`], the package
    /// forbidden; for a [`RefusalKind::ConflictsWithItself`], the package
    /// chosen.
    pub fn holder(&self) -> Option<&Package> {
        self.0.holder.as_ref()
    }

    /// How the requirement came about: first what stated it, back to the
    /// request or the world; then why the holder is installed; and where
    /// nothing can answer the name, why each package that could have was
    /// passed over. Empty for a requirement the request or the world stated
    /// on a name no record mentions.
    pub fn explanation(&self) -> &Explanation {
        &self.0.explanation
    }
}

impl fmt::Display for Refusal {
    /// Writes the requirement and why it could not be met, on one line; the
    /// [explanation](Refusal::explanation) is written on its own.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let refused = &self.0;
        let requirement = &refused.requirement;
        match &requirement.entry() {
            Some(entry) => write!(f, "{entry}")?,
            None => write!(f, "{}", requirement.name())?,
        }
        match (refused.kind, &refused.holder) {
            (RefusalKind::Conflict, Some(holder)) => write!(
                f,
                ", required by {}, conflicts with {holder}, which holds the name",
                requirement.by()
            ),
            (RefusalKind::Forbidden, Some(holder)) => write!(
                f,
                ", stated by {}, forbids {holder}, which is installed",
                requirement.by()
            ),
            (RefusalKind::ConflictsWithItself, Some(holder)) => write!(
                f,
                ", required by {}, takes {holder}, which conflicts with itself",
                requirement.by()
            ),
            (kind, _) => write!(f, ", required by {}, {kind}", requirement.by()),
        }
    }
}

impl core::error::Error for Refusal {}

/// Why a requirement could not be met, as a [`Refusal`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefusalKind {
    /// No package can answer it: none provides the name as it is required,
    /// or a decision already made rules out every one that does.
    Unmet,
    /// Only packages providing the name at no version can answer it, and
    /// none states a priority, so apk has none it may choose: it asks for
    /// one to be named instead.
    NoPriority,
    /// A package installed already provides the name, and the requirement,
    /// which binds every provider of its name, does not admit it: the
    /// [holder](Refusal::holder).
    Conflict,
    /// A `!` entry forbids a package installed already: the
    /// [requirement](Refusal::requirement) is the entry, and the
    /// [holder](Refusal::holder) the package it forbids.
    Forbidden,
    /// The package apk chose for the name, the [holder](Refusal::holder),
    /// conflicts with itself (see [`Record::conflicts_with_itself`]). For
    /// a package installed for its `install_if` condition, the requirement
    /// is on its own name, by a package completing the condition.
    ConflictsWithItself,
}

impl fmt::Display for RefusalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RefusalKind::Unmet => "which nothing can answer",
            RefusalKind::NoPriority => "provided only at no version, without a priority",
            RefusalKind::Conflict => "which conflicts with a package installed for another",
            RefusalKind::Forbidden => "which forbids a package installed for another",
            RefusalKind::ConflictsWithItself => "answered by a package conflicting with itself",
        })
    }
}

/// What required a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequiredBy {
    /// The request.
    Request,
    /// The world of the root the request is resolved over.
    World,
    /// A package installed for the request, or the only candidate for a
    /// name required, whose dependencies are required as it stands, before
    /// it is installed.
    Package(Package),
    /// Every candidate for the name given, each of which depends on the
    /// name required. An explanation goes on from it with a
    /// [`Step::Required`], which says what required the name given.
    Candidates(Name),
}

impl fmt::Display for RequiredBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequiredBy::Request => f.write_str("the request"),
            RequiredBy::World => f.write_str("the world"),
            RequiredBy::Package(package) => write!(f, "{package}"),
            RequiredBy::Candidates(name) => write!(f, "every candidate for {name}"),
        }
    }
}

/// A package an explanation or a [`Refusal`] names: its name and its
/// version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    name: Name,
    version: Version,
}

impl Package {
    fn of(record: &Record) -> Package {
        Package {
            name: record.name().clone(),
            version: record.version().clone(),
        }
    }

    /// The package's name.
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The package's version.
    pub fn version(&self) -> &Version {
        &self.version
    }
}

impl fmt::Display for Package {
    /// Writes `name-version`, as apk names a package.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

#[cfg(test)]
mod tests;
