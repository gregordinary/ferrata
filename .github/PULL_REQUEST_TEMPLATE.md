<!-- A change is best agreed in an issue before it is written; see
     CONTRIBUTING.md. Link that issue here if there is one. -->

## What this changes

<!-- The change in the repository's own terms: what the code now does. -->

## Why

<!-- The reasoning, where it is not obvious from the change itself. -->

## Checks

<!-- CI runs all of these; running them locally first is faster than a round
     trip. See CONTRIBUTING.md. -->

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] `cargo doc --workspace --all-features --no-deps` with `RUSTDOCFLAGS='-D warnings'`
- [ ] `cargo build --lib --target thumbv7em-none-eabihf`
- [ ] `cargo +1.91 check --workspace --all-targets --all-features` with `RUSTFLAGS='-D warnings'`
- [ ] `cargo deny check licenses advisories sources`, if a dependency changed
- [ ] The fuzz targets build, and those the change touches have run for a while
- [ ] The public surface is unchanged, **or** `public-api.txt` is regenerated
      and the diff is intentional

## What it resolves

- [ ] No request resolves differently, **or** the description says which,
      and the catalog holds a scenario for each rule with apk's recorded
      answer
- [ ] A difference from apk the change introduces or leaves is listed in
      the `resolve` module's documentation, with its scenario
