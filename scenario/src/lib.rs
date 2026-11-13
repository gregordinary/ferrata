//! Test tooling for ferrata: the behavior catalog's scenarios, apk.static as
//! their oracle, a reducer that shrinks a disagreement between apk and a
//! second resolver to a scenario, and the harness that holds ferrata to apk
//! and to its own properties.
//!
//! - [`format`](mod@format) reads and writes scenario files.
//! - [`archive`] writes the archive an index is served in.
//! - [`apk`] lays a scenario out and runs `apk.static` on it.
//! - [`reduce`] shrinks a scenario while apk and a second resolver disagree.
//! - [`resolve`] gives ferrata's answer to a scenario, as apk's is written.
//! - [`random`] makes choices from a seed or from a fuzzer's input.
//! - [`generate`] makes small repositories and requests at random.
//! - [`properties`] checks what every resolution holds, with no apk.
//! - [`differential`] resolves generated cases with apk and ferrata, and
//!   reduces each disagreement.
//! - [`fetch`] fetches apk.static, the keys and indexes from Alpine's
//!   mirrors.
//! - [`sweep`] resolves requests over real indexes with apk and ferrata,
//!   and compares the answers.
//!
//! The `scenario` command drives them; `scenario help` lists what it does.

pub mod apk;
pub mod archive;
pub mod differential;
pub mod fetch;
pub mod format;
pub mod generate;
pub mod properties;
pub mod random;
pub mod reduce;
pub mod resolve;
pub mod sweep;
