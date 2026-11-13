# ferrata

Alpine package dependency resolution that matches apk, in safe Rust.

ferrata is in early development, and its API is not yet stable. It resolves
a request into an empty root or over packages installed already, `!`
dependencies and `install_if` included, and orders what it changes. Its
answer is apk's but in the cases the documentation of its `resolve` module
lists, where it is known to differ.

Given the package records of Alpine repositories and a request, ferrata
chooses the packages `apk add` would install and the order it would install
them in, or refuses the request as apk would. Over an existing root it reports
the changeset apk would carry out: what it installs, upgrades, downgrades,
replaces with another build, or removes. It reads APKINDEX text and installed databases, and it
explains its answers: why each package is selected, kept or dropped, and how
the requirement a refusal could not meet came about.

Its rules are apk's, as measured: every rule is a scenario with the answer
apk-tools gives for it, one apk series at a time. It contains no `unsafe`
code, has no dependencies, and needs nothing from its platform but an
allocator. It does not fetch, decompress, or verify anything: the caller
authenticates an index before handing its records over.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
