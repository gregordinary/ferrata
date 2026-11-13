# Version vectors

apk.static's verdicts on version strings, which `tests/versions.rs` holds
ferrata to.

- `ordered.txt` holds 5,191 versions in apk's order, each with its relation
  to the one before it.
- `refused.txt` holds 741 strings apk does not read as versions.

Every relation is `apk version -t`'s answer and every refusal `apk version
-c`'s, from apk.static 3.0.9 (`apk-tools 3.0.9-r0, compiled for x86_64`),
recorded on 2026-11-09.

The strings come from three places:

- **Alpine's indexes.** Every version a package record states (`V:`), every
  version a record provides a name at (`p:`), and every version a
  dependency constrains to (`D:`, `p:` and `i:`), in the `APKINDEX` files of
  Alpine v3.0 to v3.24, edge, and postmarketOS v25.06 to v26.06 and master,
  across the architectures each serves, retrieved on 2026-11-08. That is
  29,039 distinct strings, of which apk refuses 81, all in releases before
  v3.23. `refused.txt` holds all 81. `ordered.txt` holds every version with a
  rarer feature (a commit, two suffixes, a pre-release or `cvs`, `svn` or
  `hg` suffix, a letter, a leading zero, a revision with a leading zero, or a
  number of ten digits or more), and every fortieth of the rest in sorted
  order.
- **Generated.** The first 3,000 strings `generate::strings(0, 3000)` makes.
  apk reads 2,377 of them.
- **Written by hand**, for the boundaries the module documentation states.

Over all 28,958 versions the indexes hold that apk reads, ferrata's order and
apk's agree on every adjacent pair. That measurement is not repeated here:
the indexes are not vendored.

## Checking against apk

```sh
FERRATA_APK_STATIC=/path/to/apk.static cargo test --test versions -- --nocapture
```

runs `apk_agrees`, which checks every recorded verdict against that binary,
then compares ferrata with it over 4,000 strings generated from
`FERRATA_SEED` (1 if unset). apk.static is the `apk-tools-static` package of
an Alpine release, from its `main` repository.
