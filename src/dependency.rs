//! Dependency entries: a name, and what they accept of it.
//!
//! One syntax states what a package depends on, what conflicts with it, what
//! a world file holds, and what `apk add` is asked for:
//!
//! ```text
//! [!]name[operator version]
//! [!]name><identity
//! ```
//!
//! - **The name** is every byte up to the operator: anything but a space, a
//!   line feed, and `<`, `>`, `=`, `~` and `@`. It may name a package or a
//!   name packages provide, such as `so:libc.musl-x86_64.so.1`, `cmd:sh` or
//!   `/bin/sh`. A space separates entries in a list, and apk takes any other
//!   byte, a tab, a carriage return or one that is not UTF-8 among them, as
//!   part of the name (see [`Name`]).
//! - **`!`** before the name makes the entry a conflict: no package it
//!   describes may be installed.
//! - **The operator** is a set of orderings rather than a fixed list of
//!   spellings. `<` admits an earlier version, `>` a later one, and `=` an
//!   equal one; `~` admits a version that begins with the one stated, the
//!   prefix comparison the [`version`](crate::version) module describes. The
//!   characters may come in any order and any number, so `>=` and `=>` are
//!   one operator, and `<=>` admits every version.
//! - **`><`**, or `<>`, followed by a package's identity checksum, names one
//!   build of one package. apk writes a package installed from a file into
//!   the world this way.
//! - **A name provided at no version** stands below every version: an entry
//!   whose operator admits an earlier version admits a package providing its
//!   name at no version, and any other operator does not
//!   ([`Dependency::admits_no_version`]).
//!
//! A repository tag (`name@tag`) is part of the syntax, and ferrata does not
//! read it.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use crate::name::Text;
use crate::version::{Version, VersionErrorKind};
use crate::{Dialect, Name, offset_of, split_run};

/// One dependency entry, read under one [`Dialect`].
///
/// Made by [`Dialect::dependency`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    dialect: Dialect,
    conflict: bool,
    name: Name,
    constraint: Constraint,
}

/// What an entry accepts of its name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Constraint {
    /// Any package of the name, a provider at no version included.
    Any,
    /// A package whose version the operator admits.
    Version(Operator, Version),
    /// The one package with this identity checksum, as written.
    Identity(Box<[u8]>),
}

/// The orderings an operator admits a version at, and whether it compares
/// only as far as its own version runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Operator {
    less: bool,
    equal: bool,
    greater: bool,
    prefix: bool,
}

impl Operator {
    /// Whether the operator is `><`, which names an identity rather than a
    /// version.
    fn is_identity(self) -> bool {
        self.less && self.greater && !self.equal
    }
}

impl Dependency {
    pub(crate) fn parse(dialect: Dialect, text: &[u8]) -> Result<Dependency, DependencyError> {
        // Every slice below is a tail of `text`, so an error's offset is where
        // the tail it names begins.
        let fail = |kind, at: &[u8]| {
            Err(DependencyError {
                kind,
                offset: offset_of(text, at),
            })
        };
        let (conflict, rest) = match text.strip_prefix(b"!") {
            Some(rest) => (true, rest),
            None => (false, text),
        };

        let (name, rest) = split_run(rest, |byte| {
            !is_separator(byte) && !matches!(byte, b'<' | b'>' | b'=' | b'~' | b'@')
        });
        if name.is_empty() {
            return fail(DependencyErrorKind::NoName, rest);
        }
        let (spelled, argument) = split_run(rest, |byte| matches!(byte, b'<' | b'>' | b'=' | b'~'));
        if spelled.is_empty() {
            return match rest.first() {
                None => Ok(Dependency {
                    dialect,
                    conflict,
                    name: name.into(),
                    constraint: Constraint::Any,
                }),
                Some(b'@') => fail(DependencyErrorKind::Tag, rest),
                Some(_) => fail(DependencyErrorKind::Separator, rest),
            };
        }

        let mut operator = Operator::default();
        for byte in spelled {
            match byte {
                b'<' => operator.less = true,
                b'>' => operator.greater = true,
                // `~` admits an equal version, under the prefix comparison.
                b'~' => {
                    operator.equal = true;
                    operator.prefix = true;
                }
                _ => operator.equal = true,
            }
        }
        if argument.is_empty() {
            return fail(DependencyErrorKind::Truncated, argument);
        }
        let constraint = if operator.is_identity() {
            let (_, separator) = split_run(argument, |byte| !is_separator(byte));
            if !separator.is_empty() {
                return fail(DependencyErrorKind::Separator, separator);
            }
            Constraint::Identity(argument.into())
        } else {
            match dialect.version(argument) {
                Ok(version) => Constraint::Version(operator, version),
                Err(error) => {
                    let at = argument.get(error.offset()..).unwrap_or_default();
                    return fail(DependencyErrorKind::Version(error.kind()), at);
                }
            }
        };
        Ok(Dependency {
            dialect,
            conflict,
            name: name.into(),
            constraint,
        })
    }

    /// A dependency on the empty name, which nothing provides: what apk reads
    /// for the empty entry a list holds where it begins with a space.
    pub(crate) fn on_empty_name(dialect: Dialect) -> Dependency {
        Dependency {
            dialect,
            conflict: false,
            name: Name::default(),
            constraint: Constraint::Any,
        }
    }

    /// The name the entry is about.
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// Whether the entry is a conflict, written with a leading `!`: no
    /// package it describes may be installed.
    pub fn is_conflict(&self) -> bool {
        self.conflict
    }

    /// The dialect the entry was read under.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// The identity checksum the entry names one package by, as written, if
    /// it is written `name><identity`.
    pub fn identity(&self) -> Option<&[u8]> {
        match &self.constraint {
            Constraint::Identity(identity) => Some(identity),
            Constraint::Any | Constraint::Version(..) => None,
        }
    }

    /// Whether the entry states nothing but a name, and so is answered by
    /// any package of that name or providing it, at a version or at none.
    pub(crate) fn is_unconstrained(&self) -> bool {
        matches!(self.constraint, Constraint::Any)
    }

    /// Whether the entry's constraint admits a package of its name at
    /// `version`.
    ///
    /// An entry with no operator admits every version, and an entry naming a
    /// package by its identity admits none, since a version alone does not
    /// establish an identity. The `!` plays no part: a conflict forbids what
    /// its constraint admits.
    ///
    /// ```
    /// use ferrata::Dialect;
    ///
    /// let entry = Dialect::V3_0.dependency("python3~3.12")?;
    /// assert!(entry.admits(&Dialect::V3_0.version("3.12.11-r0")?));
    /// assert!(!entry.admits(&Dialect::V3_0.version("3.13.0-r0")?));
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub fn admits(&self, version: &Version) -> bool {
        match &self.constraint {
            Constraint::Any => true,
            Constraint::Identity(_) => false,
            Constraint::Version(operator, wanted) => {
                let order = if operator.prefix {
                    version.compare_prefix(wanted)
                } else {
                    version.cmp(wanted)
                };
                match order {
                    Ordering::Less => operator.less,
                    Ordering::Equal => operator.equal,
                    Ordering::Greater => operator.greater,
                }
            }
        }
    }

    /// Whether the entry admits a package providing its name at no version.
    ///
    /// No version stands below every version, so an entry admits it where
    /// its operator admits an earlier version than the one stated, as `<`,
    /// `<=` and `<~` do, and where it states nothing but the name. An entry
    /// naming an identity admits none.
    ///
    /// ```
    /// use ferrata::Dialect;
    ///
    /// assert!(Dialect::V3_0.dependency("so:libz.so.1<2")?.admits_no_version());
    /// assert!(Dialect::V3_0.dependency("so:libz.so.1")?.admits_no_version());
    /// assert!(!Dialect::V3_0.dependency("so:libz.so.1>=1")?.admits_no_version());
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub fn admits_no_version(&self) -> bool {
        match &self.constraint {
            Constraint::Any => true,
            Constraint::Identity(_) => false,
            Constraint::Version(operator, _) => operator.less,
        }
    }

    /// The entry as bytes, in the canonical spelling: what
    /// [`Dialect::dependency`] reads back as this entry, under its dialect,
    /// its name and identity byte for byte, whatever bytes they hold.
    ///
    /// The operator is rebuilt from the orderings it admits: `<` or `>`
    /// first, then `=` or `~`, so `=>` is written `>=`, `=~` is written `~`,
    /// and an identity's operator is written `><`. This is the spelling apk
    /// writes into a world file, but for an operator admitting every
    /// ordering: apk writes `<=>` as no operator at all and `<>~` as `?`,
    /// which no reader takes back, and ferrata writes them `<>=` and `<>~`.
    ///
    /// A dependency on the empty name, which an index's record holds where
    /// a list begins with a space, is written as nothing, which does not
    /// read: a list writes it by beginning with a space.
    ///
    /// ```
    /// use ferrata::Dialect;
    ///
    /// let entry = Dialect::V3_0.dependency(b"!lib\xff=>2.0")?;
    /// assert_eq!(entry.to_bytes(), b"!lib\xff>=2.0");
    /// assert_eq!(Dialect::V3_0.dependency(entry.to_bytes())?, entry);
    /// # Ok::<(), ferrata::dependency::DependencyError>(())
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        self.spelling()
            .flat_map(|part| part.iter().copied())
            .collect()
    }

    /// The parts [`Dependency::to_bytes`] joins: the `!`, the name, the
    /// operator, and the version or identity, each possibly empty. Every
    /// part but the name and the identity is ASCII, which no sequence of
    /// UTF-8 continues into, so `Display` escaping part by part writes what
    /// escaping the whole would.
    fn spelling(&self) -> impl Iterator<Item = &[u8]> {
        let empty: &[u8] = b"";
        let conflict: &[u8] = if self.conflict { b"!" } else { empty };
        let (operator, argument): ([&[u8]; 3], &[u8]) = match &self.constraint {
            Constraint::Any => ([empty; 3], empty),
            Constraint::Identity(identity) => ([b"><", empty, empty], identity),
            Constraint::Version(operator, version) => (
                [
                    if operator.less { b"<" } else { empty },
                    if operator.greater { b">" } else { empty },
                    if operator.prefix {
                        b"~"
                    } else if operator.equal {
                        b"="
                    } else {
                        empty
                    },
                ],
                version.as_bytes(),
            ),
        };
        [conflict, self.name.as_bytes()]
            .into_iter()
            .chain(operator)
            .chain([argument])
    }
}

impl fmt::Display for Dependency {
    /// Writes the entry as [`Dependency::to_bytes`] spells it, as
    /// [`Name`] writes a name: UTF-8 as it is, each byte that is not UTF-8
    /// as `\xNN`, and a backslash as `\\`. An entry whose name and identity
    /// hold neither is written as its bytes are.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.spelling()
            .try_for_each(|part| fmt::Display::fmt(&Text(part), f))
    }
}

/// Whether `byte` separates one entry from the next, in a list of
/// dependencies or in a world file.
fn is_separator(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n')
}

/// Text a [`Dialect`] does not read as a dependency entry: what was wrong,
/// and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DependencyError {
    kind: DependencyErrorKind,
    offset: usize,
}

impl DependencyError {
    /// What was wrong.
    pub fn kind(&self) -> DependencyErrorKind {
        self.kind
    }

    /// The byte offset in the text at which the failure was found. A
    /// failure found at the end of the text names its length.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

impl fmt::Display for DependencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a dependency: {} at byte {}", self.kind, self.offset)
    }
}

impl core::error::Error for DependencyError {}

/// What was wrong with a dependency entry, as a [`DependencyError`] reports
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DependencyErrorKind {
    /// The entry names nothing: it is empty, holds only `!`, or begins with
    /// an operator.
    NoName,
    /// The entry holds a space or a line feed, which separate entries rather
    /// than belonging to one.
    Separator,
    /// The entry names a repository tag, which ferrata does not read.
    Tag,
    /// An operator with nothing after it.
    Truncated,
    /// The text after the operator is not a version.
    Version(VersionErrorKind),
}

impl fmt::Display for DependencyErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DependencyErrorKind::NoName => f.write_str("no name"),
            DependencyErrorKind::Separator => f.write_str("a space or a line feed"),
            DependencyErrorKind::Tag => f.write_str("a repository tag"),
            DependencyErrorKind::Truncated => f.write_str("an operator with nothing after it"),
            DependencyErrorKind::Version(kind) => write!(f, "{kind} in the version"),
        }
    }
}

#[cfg(test)]
mod tests;
