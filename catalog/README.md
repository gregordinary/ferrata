# The behavior catalog

The rules ferrata follows, each shown by a scenario: a statement of the rule,
the smallest repositories that show it, a request, and the answer apk gave.
The catalog is the specification ferrata's resolver is written against, and
the answers are apk's, not ferrata's.

| Directory | What its scenarios show |
| --- | --- |
| `versions/` | which of the versions offered a constraint chooses |
| `providers/` | how the candidates for one name are ranked |
| `repositories/` | what the order of several repositories decides |
| `decision-order/` | which names apk decides first, and what that changes |
| `install-if/` | when a package is installed for its `install_if` |
| `conflicts/` | what a `!` dependency forbids |
| `request/` | how a request is read |
| `installed/` | a request over packages already installed |
| `install-order/` | the order apk installs a resolution in |
| `refusals/` | which requests apk refuses rather than look further |
| `index/` | how apk reads an index, malformed ones included |
| `reduced/` | real cases, reduced to the records that decide them |

## A scenario

A scenario file is text in sections:

```text
[statement]
A package is installed after what it depends on.

[repository]
P:a
V:1
D:b

P:b
V:1

[request]
a

[answer apk-tools 3.0.9-r0]
install b 1
install a 1
```

- `[statement]` states the rule in prose. A file that records an
  observation whose rule is not yet known says so.
- `[repository]` holds package records in `APKINDEX` syntax, separated by
  blank lines; each section is one repository, in the order apk is given
  them. A record without `C:` is given an identity derived from its name and
  version, so records of one name and version are one package unless they
  state their own identities. `[repository verbatim]` holds an index's text
  line by line, for scenarios about text apk cannot read: its lines up to
  the last that is not blank, each ending in a line feed. A line that would
  open a section (`[`, anything, `]`) cannot be held, nor blank lines at
  the end of the text, and an empty section is one line feed.
- `[installed]` and `[world]` describe a root already installed: its
  packages' records, and its world, one entry to a line.
- `[request]` holds what is asked of `apk add`, one argument to a line.
- `[answer ...]` is apk's answer, the header naming the apk that gave it:
  the changeset in order (`install`, `upgrade`, `downgrade`, `replace`,
  `reinstall`, `remove`, with the name and the version or versions), or
  `refused`; and
  anything else apk printed, each line after `| `.

A line beginning `# ` is a comment, except in a verbatim repository.

## The `scenario` tool

The `scenario` crate in this repository runs scenarios with `apk.static`,
the `apk-tools-static` package of an Alpine release. It needs no privilege:
it writes the root, an empty world and installed database, by hand, and
runs `apk add --simulate` against unsigned indexes.

```sh
export FERRATA_APK_STATIC=/path/to/apk.static
cargo run -p ferrata-scenario -- run catalog/install-order/each-package-after-what-it-depends-on.scenario
cargo run -p ferrata-scenario -- record catalog/conflicts/a-new-rule.scenario
cargo run -p ferrata-scenario -- check catalog
```

`record` writes apk's answer into the file; `check` reports every scenario
whose recorded answer is no longer apk's. `cargo test --workspace` checks
that every scenario is well formed, and with `FERRATA_APK_STATIC` set, asks
apk again.

`scenario reduce` shrinks a scenario on which apk and a second resolver
disagree. The second resolver is any command: the reducer appends the path
of a directory holding the scenario's indexes and request, as `scenario
materialize` lays them out, and reads the command's answer, written as an
answer section writes one. The reducer removes records, fields, and list
entries while the two still disagree in the same way, and `--keep NAME`
holds a package on the side of the disagreement it began on. `scenario new`
makes a scenario from real indexes to start from.

## Adding a scenario

Write the statement, the repositories, and the request; run `record`; read
the answer, and check that the statement says what it shows. Name the file
for the rule, in a few words, and put variants of one rule beside it with a
suffix that says what differs.
