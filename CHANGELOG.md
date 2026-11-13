# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html): while the
major version is 0, a breaking change bumps the minor version. An entry a
caller must act on is prefixed **Breaking.**

A change to what any request resolves to is a minor version of its own,
recorded even when no signature of the public API changes, since what
ferrata resolves is part of its contract. Its entry states how the answers
over each Alpine release measured change: how many names resolve otherwise,
and in what way.

## [0.1.0] - 2026-11-13

The first release, following apk-tools 3.0 (`Dialect::V3_0`), measured
against apk-tools 3.0.9, which Alpine v3.23, v3.24 and edge ship.

### Added

- `Dialect`, the apk series whose rules answer a question, and
  `Dialect::for_release`, which finds the one a release's `apk-tools`
  follows and says whether its rules were measured against that release.
- `version`: apk's version grammar and ordering, and the versions a package
  record may hold outside it, read and ordered as apk reads them.
- `dependency`: dependency entries as packages, worlds and requests state
  them, and which versions an entry admits.
- `record` and `index`: package records read from `APKINDEX` text and from
  an installed database, as bytes, each with the text it was read from.
- `resolve`: the packages `apk add` installs for a request, into an empty
  root or over the packages a root holds, with `install_if`, `!` entries and
  provider priorities; the changes it makes to the root, in the order it
  makes them, each naming the repository of the package it installs; and
  why each package is in a resolution, dropped from the root, or why a
  request is refused.

### Agreement with apk

Measured against apk.static 3.0.9 over Alpine v3.24 (`main` and `community`):

- Every name requested alone: 28,660 of 28,661 in apk's exact sequence; the
  other installs apk's packages in another order. Over Alpine v3.23, 27,574
  of 27,577: one in another order, and two with another set of packages.
- 2,000 layered pairs, a name installed into an empty root and another
  requested over it: 2,000 as apk answers them.
- 2,000 layered pairs whose root is installed from Alpine v3.23: 1,999 as
  apk answers them, and one apk refuses where ferrata answers. 2,000 such
  pairs whose request also upgrades the first name: 1,995, 4 in another
  order, and one apk refuses where ferrata answers.
- 15,000 generated repositories and requests: 306 answered otherwise, 2.0%.

The documentation of the `resolve` module lists the 28 cases known to differ
from apk.

[0.1.0]: https://github.com/gregordinary/ferrata/releases/tag/v0.1.0
