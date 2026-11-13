//! Alpine package dependency resolution that matches apk, in safe Rust.
//!
//! Given the package records of Alpine repositories, a request, and the
//! packages a root holds already, if any, ferrata answers with the changes
//! `apk add` would make to the root, in the order it would make them, with
//! no dependencies and no `unsafe` code. Its rules are those of apk-tools as
//! measured, one series at a time. It is in early development.
//!
//! Every question is asked of a [`Dialect`], the apk series whose rules
//! answer it:
//!
//! - [`dialect`] names the series, and finds the one a release's apk-tools
//!   follows.
//! - [`version`] reads package versions and orders them.
//! - [`dependency`] reads dependency entries, and says which versions one
//!   admits.
//! - [`record`] holds what a repository or a root states about one package.
//! - [`index`] reads records from an `APKINDEX` or an installed database.
//! - [`resolve`] chooses the packages a request installs, upgrades and
//!   removes, and orders them.
//!
//! apk reads its indexes, its installed database, its world and its
//! arguments as bytes, and so does ferrata: a package's name is a [`Name`],
//! the bytes apk compares, and what ferrata only carries is the bytes it was
//! read from.
//!
//! The crate is `no_std` and needs only an allocator.

#![no_std]
// Package indexes come from mirrors and installed databases from roots this
// crate does not control, so a panic in library code is a decision made where
// the code is written rather than a finding a fuzzer reports later. Each lint
// below names a way to panic, or to compute a wrong length silently; a site
// that genuinely cannot fail and is clearer written directly carries
// `#[expect(..., reason = "...")]` on the smallest item that holds it, with the
// reason stating the invariant. Tests are exempt: there a panic is how a check
// fails.
#![cfg_attr(
    not(test),
    deny(
        clippy::alloc_instead_of_core,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::std_instead_of_alloc,
        clippy::std_instead_of_core,
        clippy::string_slice,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used,
    )
)]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod dependency;
pub mod dialect;
pub mod index;
mod name;
pub mod record;
pub mod resolve;
pub mod version;

pub use dialect::Dialect;
pub use name::Name;
pub use resolve::Resolver;

/// The byte offset at which `tail`, which ends `whole`, begins in it.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`tail` ends `whole`, so it is never the longer"
)]
fn offset_of(whole: &[u8], tail: &[u8]) -> usize {
    whole.len() - tail.len()
}

/// Splits `text` after the longest run at its start that `wanted` accepts.
fn split_run(text: &[u8], wanted: impl Fn(u8) -> bool) -> (&[u8], &[u8]) {
    let length = text
        .iter()
        .position(|&byte| !wanted(byte))
        .unwrap_or(text.len());
    text.split_at_checked(length).unwrap_or((text, &[]))
}
