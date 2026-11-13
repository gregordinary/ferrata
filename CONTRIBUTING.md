# Contributing

ferrata is maintained in spare time. This document sets out how changes are
handled, and what you need to build and test the code.

## Reporting a security issue

Report a resolution that breaks a constraint its input states, a panic or
runaway work on hostile input, or another security issue, privately,
through the channel [SECURITY.md](SECURITY.md) describes. Please do not open
a public issue for one.

## Proposing a change

Open an issue before writing code, so the design can be agreed before anyone
spends time on it. What ferrata resolves is as much a part of its contract as
its public API: a change in what any request resolves to, even one that fixes
a divergence from apk, is a release of its own, recorded in the changelog with
what it changes.

## Matching apk

ferrata's rules are apk-tools' rules as observed, not as written. Each rule is
specified by measurement: a scenario this project writes, and the answer
`apk.static` gives for it. A question about what apk does is answered by
running apk.

apk-tools is licensed GPL-2.0-only, and ferrata is MIT or Apache-2.0. ferrata
is written from scratch: no part of apk-tools is translated into it, and its
source is not read while ferrata's code is written. Please follow the same
rule in any contribution. A situation apk-tools' own tests cover is a fine
thing to measure; their files are not copied.

A difference between ferrata and apk on any input is a defect, unless the
documentation states it.

The rules are recorded in `catalog/`, one scenario to a file: the rule in
prose, the repositories, the request, and apk's answer.
[catalog/README.md](catalog/README.md) describes the format and the
`scenario` tool that runs a scenario with apk, records its answer, and
shrinks a disagreement between apk and another resolver to a scenario. A
change to what ferrata resolves comes with the scenarios that show it.

The tests hold ferrata to apk's recorded answers, and need no apk to run.
The scenarios ferrata is known to answer otherwise are listed in the
`resolve` module's documentation, under "Where ferrata answers otherwise",
each with the behavior it lacks; the catalog test reads its list from there.
It fails on any other difference, on a listed scenario ferrata comes to
answer as apk does, and on a listed path that is no scenario.
With `FERRATA_APK_STATIC` naming an `apk.static` binary, the tests that
compare with apk also ask it again, and over input generated at random:

```sh
FERRATA_APK_STATIC=/path/to/apk.static cargo test --workspace -- --nocapture
```

apk.static is the `apk-tools-static` package of an Alpine release, in its
`main` repository. Each recorded answer names the apk version that gave it,
and the README beside it says where its input came from.

The one the tooling compares with is pinned in `scenario/src/fetch.rs`, by
release, version and SHA-256, beside the `alpine-keys` package whose keys
verify indexes. Neither is kept in the repository: apk-tools is licensed
GPL-2.0-only, and the tooling fetches the binary package when it is needed.

```sh
cargo run --release -p ferrata-scenario -- fetch work
```

fetches both into `work/`, checks each against its pin, and fetches the
pinned release's `main` and `community` indexes as their mirror serves
them, signed; apk.static verifies each with the keys as it loads it. Other
repositories are named by URL, in the order a root is configured with them,
with `--key` for a repository signed by other keys and `--arch` for another
architecture. `--release-apk` also fetches the apk.static the repositories
offer, where it is not the pinned one, and has the pinned one verify it. A
directory is reused as it is, and its pinned apk.static runs only once its
digest is the one the pinned package fixes, pinned beside it.

The sweep resolves requests over those indexes with apk.static and with
ferrata in one run, and compares the answers, set and sequence: every name
the repositories carry, alone, or with `--sets COUNT --size K --seed S`,
COUNT sets of K names drawn at random from the seed, each resolved
together, which reaches what no single name does. With `--layered COUNT
--seed S`, it resolves COUNT layered pairs instead: a name apk installs
into an empty root becomes the root's installed database and world, and a
second name is requested over it, so the answers compared are changesets
that upgrade and remove as well as install. `--base DIR` installs the first
name from another fetched directory, an older release's repositories, and
`--upgrade` also asks for it at a later version where one is offered.

```sh
cargo run --release -p ferrata-scenario -- sweep work --jobs 12 -o v3.24.txt
cargo run --release -p ferrata-scenario -- summary v3.24.txt
```

Each results file names the commit ferrata was built from, the apk that
answered and its binary's digest, and each index's description and digest,
and holds both answers to every request; `scenario/src/sweep.rs` describes
the format. `--shard I/N` runs one of N shards, so a long sweep can run as
several jobs: a name resolved alone falls in the shard a hash of it selects,
whatever other names the repositories hold, and drawn sets and layered pairs
are dealt out among the shards in the order their seed draws them. `summary`
reads the shards of one run together, and fails where one is missing, given
twice, or cut short. `compare --before FILE... --after FILE...` reports the
requests whose apk answer or ferrata answer changed between two runs. `case DIR REQUEST`
writes the scenario of one request of a results file, every record of the
repositories with the root a layered pair's base makes, for `prune` and
`reduce` to shrink to the rule behind a difference; `reduce --consistent`
keeps the root one apk could have made as it shrinks. One release takes about
twenty minutes at twelve requests at a time. With `FERRATA_SWEEP` naming a
directory, the sweep runs as a test, and fetches into the directory first
where it is empty:

```sh
FERRATA_SWEEP=work cargo test --release -p ferrata-scenario --test sweep -- --nocapture
```

The drift workflow (`.github/workflows/drift.yml`) runs weekly, never on a
pull request. Where the pinned release has come to offer another
apk.static, it asks that apk the catalog's scenarios again and regenerates
their answers, sweeps the release with both apks in eight shards, and
reports which answers changed: the measurement a release of ferrata that
follows the new apk starts from. The release's mirror drops a pinned
package once it offers another build of it. The workflow keeps the pinned
keys in a cache, keyed by their digest, and `fetch` takes a package already
in the directory where its digest is the pin's. apk-tools is GPL-2.0-only,
and a fork's pull request can restore the default branch's caches, so the
pinned apk-tools-static is never cached: once the mirror drops it, the run
fails at fetching, its summary saying the pin must move, and the two apks
are compared from a fetched directory that still holds the pinned package,
with `sweep --apk` and `compare`.

Real indexes are not vendored. With `FERRATA_INDEXES` naming a directory of
`APKINDEX` files, taken out of their archives, `tests/indexes.rs` reads every
one, and checks that each record writes back as the text it was read from:

```sh
FERRATA_INDEXES=/path/to/indexes cargo test --release --test indexes -- --nocapture
```

## Building and testing

The toolchain is pinned in `rust-toolchain.toml`, so `rustup` provisions the
right version, components, and targets automatically.

```sh
cargo build
cargo test
```

The library has no dependencies.

## Before opening a pull request

CI runs all of these, and running them locally first is faster than a round
trip.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps    # RUSTDOCFLAGS='-D warnings'
cargo build --lib --target thumbv7em-none-eabihf
cargo +1.91 check --workspace --all-targets --all-features    # RUSTFLAGS='-D warnings'
cargo deny check licenses advisories sources
cargo deny --manifest-path fuzz/Cargo.toml check licenses advisories sources
```

The fifth builds the library for a target with no standard library. The sixth
checks the minimum supported Rust version, which sits below the pinned
toolchain, and needs that toolchain installed. The last two check the
licenses, advisories and sources of the workspace's dependencies and of the
fuzzing harness's, which are the only third-party crates, and need
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny).

The public API is recorded in `public-api.txt`, and CI fails on any change to
it that the file does not show. A change that adds to or alters the API
regenerates it with
[cargo-public-api](https://github.com/cargo-public-api/cargo-public-api),
under the nightly the CI job pins:

```sh
RUSTUP_TOOLCHAIN=nightly-2026-07-09 cargo public-api -p ferrata --all-features \
    --omit blanket-impls,auto-derived-impls > public-api.txt
```

CI's `semver` job builds the newest release tag and the tree, and runs
[cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks)
between them. While `version` still names the last release, it fails on a
breaking change and lists the items it breaks. Bumping the version to the
next breaking release, 0.1.x to 0.2.0, turns it green, and each item it
listed takes a **Breaking.** entry in CHANGELOG.md. The job sees only the
API's signatures: a change in what a request resolves to is a release of its
own even when the job passes.

## Properties and fuzzing

Some of what ferrata answers can be checked without apk: every resolution
meets its own requirements, violates no `!` entry, installs one package to
each name held, installs nothing that nothing asks for, places each package
after what it depends on but at a cycle, and gives the same answer twice.
`scenario/src/properties.rs` checks these, and `scenario/tests/properties.rs`
runs the checks on every `cargo test`, over small repositories generated at
random. A failure names the seed that made the case, and `scenario generate
SEED` writes it out as a scenario.

`fuzz/` holds a [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) target
for each reader of untrusted input, and two for the resolver, which hold
every answer to the same checks: `resolve` reads its input as an index, and
`generated` steers the generator of small repositories with it. The targets
need a nightly toolchain:

```sh
cd fuzz
cargo +nightly fuzz run versions
```

`cargo fuzz list` names the targets. CI builds every target but runs none.
After changing a reader or the resolver, run its target for a while before
opening the pull request.

The differential fuzzer resolves the same generated cases with apk.static
and with ferrata, and shrinks each disagreement to a scenario with the
reducer, ferrata being the second resolver:

```sh
FERRATA_APK_STATIC=work/apk.static \
    cargo run --release -p ferrata-scenario -- fuzz --count 10000 --reduce found
```

A disagreement found this way is a rule ferrata lacks. It goes into the
catalog as a scenario stating what apk does, and into the `resolve`
module's known differences until ferrata follows it.

## Code conventions

- **No `unsafe`.** The crate forbids it.
- **No panics in library code.** Indexes come from mirrors and installed
  databases from roots the caller does not control, so a reachable panic is a
  denial of service waiting for an input to find it. The lints at the top of
  `src/lib.rs` deny the common ways to panic or to compute a wrong length
  silently: indexing, unchecked arithmetic, truncating casts, `unwrap`,
  `expect`, and the panicking macros. A site that genuinely cannot fail, and
  is clearer written directly, carries `#[expect(lint, reason = "...")]` on
  the smallest item that holds it, with the reason stating the invariant that
  makes it safe. Tests are exempt.
- **Bounded work.** Nothing recurses on the structure of its input: a walk
  that follows dependencies keeps an explicit stack, since a closure can be
  thousands of packages deep. Reading is linear in its input. The solver
  searches nothing. The decisions it makes after its preliminary choices
  cost at most time quadratic in the records: it comes to each name and
  each turn a bounded number of times, and what it does for each, and for
  each requirement it applies and each package it installs or rules out,
  costs at most time linear in the records over a resolution. A question
  asked again at each step keeps what its last answer settled, rather
  than looking through the records afresh.
- **Deterministic answers.** A resolution is a function of its inputs' content
  and of nothing else. Collections keyed by name are `BTreeMap` and
  `BTreeSet`, whose iteration order is a property of the data. Every list the
  input states in an order keeps that order: a record's dependencies,
  provides, and `install_if`, the request, and the repositories. apk's answer
  can depend on any of them, so holding one as a set discards a fact apk
  uses.
- **`no_std` with `alloc`.** The library needs an allocator and nothing else
  from its platform.
- **Errors are typed and say where.** A read error names the record and the
  line its problem was found on. Error enums are `#[non_exhaustive]`, so a new
  kind of failure is not a breaking change.
- **An answer's enums are exhaustive.** `Change`, `Source`, `RefusalKind`,
  and an explanation's `Step`, `Reason`, `Exclusion` and `RequiredBy` are
  what a caller acts on, case by case. A new variant comes only with a
  change in what ferrata resolves or explains, which is a release of its
  own, so it is a breaking change made openly: a caller's match stops
  compiling rather than meeting a case it cannot handle. The library's own
  matches on them name every variant, so a new one is met wherever it
  matters.
- **Documentation is part of the change.** Public items carry rustdoc, and
  internal items carry it wherever the behavior is not obvious. Comments
  explain *why*, not *what*. A rule taken from apk's behavior names the
  scenario that shows it.
- **No emoji** in code, comments, documentation, or commit messages.

## Commit messages

Write in the imperative mood, describing the change in the repository's own
terms. A body explaining the reasoning is welcome where the reasoning is not
obvious.

## License

The project is dual-licensed under Apache-2.0 and MIT. Unless you state
otherwise, any contribution you intentionally submit for inclusion, as defined
in the Apache-2.0 license, is dual-licensed the same way, with no additional
terms or conditions.
