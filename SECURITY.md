# Security policy

ferrata decides which packages a root gets. It reads package indexes from
mirrors and installed databases from roots its caller does not control, and
its answer says what the caller fetches and installs. A defect can mean a
package the input forbids is installed, or that ferrata stops answering on
input crafted to make it, so security reports are handled separately from
ordinary bugs.

## Reporting a vulnerability

**Do not open a public issue.** Report privately through GitHub's [private
vulnerability reporting](https://github.com/gregordinary/ferrata/security/advisories/new)
for this repository, which opens a draft advisory visible only to you and the
maintainers.

Please include, as far as you can:

- The input: the index records, the installed database and world if there
  is a root, and the request. A catalog scenario (see
  [CONTRIBUTING.md](CONTRIBUTING.md)) holds all of them in one file, and
  records made for the report serve as well as real ones.
- What ferrata returned, and what it should have returned. If apk answers
  the input differently, what `apk add --simulate` prints, and its version.
- The commit or version of ferrata.

You will get an acknowledgement within a few days. Once a fix exists, the
advisory names the reporter unless you ask otherwise.

## What is in scope

- **A resolution that breaks a constraint its input states.** A package
  installed that a `!` entry of the request, the world, or a package
  installed forbids; a package installed at a version an entry does not
  admit; or a dependency of a package installed that nothing installed
  answers.
- **A package the input does not hold.** A resolution or a change naming a
  record that no repository offered and the root did not hold, or one whose
  name, version, or identity differs from the text it was read from, so
  that a caller would fetch or verify something other than what the index
  states.
- **Panics and runaway work on hostile input.** The library is written not
  to panic and not to recurse on the structure of its input. A panic, a
  stack overflow, or time or memory that grows faster than polynomially in
  the records it is given, on any index, installed database, or request, is
  a vulnerability, since indexes and roots come from outside the caller's
  control. CONTRIBUTING.md states the bound the solver keeps.

## What is not in scope

- **The documented differences from apk.** The documentation of the
  `resolve` module lists the cases where ferrata's answer is known to
  differ from apk's. An answer that differs from apk's in a case it does not
  list is a defect, and welcome as an ordinary issue, unless it is one of
  the cases above.
- **Authenticating input.** ferrata does not fetch, decompress, or verify
  anything: the caller authenticates an index before handing its records
  over. A forged index that ferrata reads as it is written is the caller's
  to refuse.

If you are unsure which side of the line a finding falls on, report it
privately and we will work it out.

## Supported versions

Security fixes are made to the newest release, and published as a release
of their own; earlier releases are not patched, so receiving a fix means
upgrading to the newest release. While the major version is 0, a fix that
breaks compatibility with the public API is published as a new minor
version, such as 0.2.0 after 0.1.x, and any other, including one that only
adds to the API, as a new patch version. A fix that changes what a request
resolves to is a minor version, as every change in resolution is.
