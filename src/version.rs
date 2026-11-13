//! Package versions: what one may be, and how two compare.
//!
//! # What a version may be
//!
//! An apk version is a sequence of parts, each of which may appear only after
//! the parts the grammar places before it:
//!
//! ```text
//! 1.2.3a_rc2_p~0ff1ce-r4
//! ^ ^ ^^^  ^ ^ ^      ^
//! | | ||   | | |      revision: `-r` and digits
//! | | ||   | | commit: `~` and lowercase hexadecimal digits
//! | | ||   | a further suffix
//! | | ||   the suffix's number, which it may lack
//! | | |a suffix: `_` and a name from the table below
//! | | a letter: one lowercase ASCII letter
//! | dotted numbers, as many as wanted
//! the first number
//! ```
//!
//! Every part but the first number is optional. The dotted numbers and the
//! suffixes repeat, and the rest appear at most once. Every part is ASCII:
//! a version is read as bytes, and a byte the grammar has no place for,
//! whether ASCII or not, is out of place. A number is a run of
//! ASCII digits whose value is at most 2^64 - 1; leading zeros are allowed.
//! A suffix's name is one of these, in the order they rank:
//!
//! ```text
//! alpha < beta < pre < rc < (none) < cvs < svn < git < hg < p
//! ```
//!
//! The first four mark a pre-release, which comes before the version without
//! them; the rest mark a release after it. Text with any part out of place,
//! such as `1.0-r1_p1` (a suffix after the revision) or `1.0a1` (digits after
//! the letter), is not a version.
//!
//! # How two versions compare
//!
//! Two versions are compared part by part from the start. While the parts are
//! of one kind, the first pair that differs decides:
//!
//! - numbers by value, except that a dotted number is compared as text when
//!   either of the two begins with `0`, so `1.05` comes before `1.5` and
//!   `1.00` after `1.0`; the first number is always compared by value;
//! - letters alphabetically, and suffixes by the table above;
//! - a suffix's number and a revision by value;
//! - commits as text.
//!
//! Where one version has a part of a different kind from the other's, or has
//! run out, a pre-release suffix makes its version the earlier. Otherwise the
//! version whose part comes earlier in the grammar is the later, and having
//! run out comes after every kind of part:
//!
//! ```text
//! 1.0_rc1 < 1.0 < 1.0-r1 < 1.0~ff < 1.0_p1 < 1.0a < 1.0.1
//! ```
//!
//! A suffix's number is a part of its own, so after a shared suffix, a
//! version with a number meets one without: `1.0_p` comes before `1.0_p0`,
//! and `1.0_p1` before `1.0_p_p`, where a further suffix meets the number.
//!
//! Versions that compare equal are one version however they are spelled:
//! `1.0-r01` is `1.0-r1`, and `01.0` is `1.0`.
//!
//! # Versions outside the grammar
//!
//! [`Dialect::version`] reads only what the grammar allows, as `apk version
//! -c` and `apk add` do. A package record is another matter: apk loads a
//! record whose version the grammar does not allow, such as `1.0b1-r0`
//! (digits after the letter), installs it, and orders it against others, so
//! a record read by [`index`](crate::index) keeps such a version, bytes that
//! are not UTF-8 included, and [`Version::is_valid`] tells it apart.
//! [`Dialect::recorded_version`] reads one the same way, for a version that
//! comes from somewhere other than an index.
//!
//! apk orders such a version as far as it can read it. At the part it cannot
//! read, the version comes before any other that continues there, and before
//! one that has run out, unless that one's part is a pre-release suffix. Two
//! versions that go wrong at the same part, after equal parts, are equal:
//!
//! ```text
//! 1.0_rc1 < 1.0_foo < 1.0 < 1.0-r0 < 1.0b1-r0 = 1.0b1-r9 < 1.0b < 1.0c
//! ```

use alloc::boxed::Box;
use core::cmp::Ordering;
use core::fmt;

use crate::name::Text;
use crate::{Dialect, offset_of, split_run};

/// A package version, read under one [`Dialect`].
///
/// [`Ord`] is the dialect's ordering, and equality is that ordering's:
/// `1.0-r01` and `1.0-r1` are equal, though they are spelled apart. Versions
/// read under different dialects order by dialect first.
///
/// Made by [`Dialect::version`], and by [`Dialect::recorded_version`] and
/// the [`index`](crate::index) reader, which keep a version even where it is
/// not [valid](Version::is_valid).
#[derive(Clone)]
pub struct Version {
    dialect: Dialect,
    text: Box<[u8]>,
    valid: bool,
}

impl Version {
    pub(crate) fn parse(dialect: Dialect, text: &[u8]) -> Result<Version, VersionError> {
        validate(text)?;
        Ok(Version {
            dialect,
            text: text.into(),
            valid: true,
        })
    }

    /// Reads any text as a version, as apk reads a record's: text the
    /// grammar does not allow is kept, and ordered as far as it can be read.
    pub(crate) fn read(dialect: Dialect, text: &[u8]) -> Version {
        Version {
            dialect,
            text: text.into(),
            valid: validate(text).is_ok(),
        }
    }

    /// The version as it was written.
    pub fn as_bytes(&self) -> &[u8] {
        &self.text
    }

    /// The version as it was written, as text, where it is UTF-8: always,
    /// for a [valid](Version::is_valid) version, which is ASCII.
    pub fn to_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.text).ok()
    }

    /// Whether the version is one the grammar allows, as [`Dialect::version`]
    /// requires. Only a version read as a package record states it can be
    /// otherwise ([`Dialect::recorded_version`]).
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// The dialect the version was read under.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Compares this version with `other` as far as `other` runs, which is the
    /// comparison a `~` constraint makes: where `other` has run out and this
    /// version continues, the two are equal.
    pub(crate) fn compare_prefix(&self, other: &Version) -> Ordering {
        compare(&self.text, &other.text, Extent::Prefix)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        self.dialect
            .cmp(&other.dialect)
            .then_with(|| compare(&self.text, &other.text, Extent::Whole))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    /// Whether the two are one version by the ordering, rather than by their
    /// spelling, which keeps [`Ord`]'s contract.
    fn eq(&self, other: &Version) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Version {}

impl fmt::Display for Version {
    /// Writes the version as it was written, which for a
    /// [valid](Version::is_valid) version is ASCII. Another is written as
    /// [`Name`](crate::Name) writes a name: UTF-8 as it is, each byte that is
    /// not UTF-8 as `\xNN`, and a backslash as `\\`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&Text(&self.text), f)
    }
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Version")
            .field("dialect", &self.dialect)
            .field("text", &Text(&self.text))
            .field("valid", &self.valid)
            .finish()
    }
}

/// Text a [`Dialect`] does not read as a version: what was wrong, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionError {
    kind: VersionErrorKind,
    offset: usize,
}

impl VersionError {
    /// What was wrong.
    pub fn kind(&self) -> VersionErrorKind {
        self.kind
    }

    /// The byte offset in the text at which the failure was found. A
    /// failure found at the end of the text names its length.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a version: {} at byte {}", self.kind, self.offset)
    }
}

impl core::error::Error for VersionError {}

/// What was wrong with a version, as a [`VersionError`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VersionErrorKind {
    /// The text is empty.
    Empty,
    /// A character the grammar has no place for at this point, or the end of
    /// the text where a part was not finished.
    Unexpected,
    /// A suffix whose name is not one the dialect defines.
    UnknownSuffix,
    /// A number greater than 2^64 - 1.
    TooLarge,
}

impl fmt::Display for VersionErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            VersionErrorKind::Empty => "no text",
            VersionErrorKind::Unexpected => "a character out of place",
            VersionErrorKind::UnknownSuffix => "an unknown suffix",
            VersionErrorKind::TooLarge => "a number greater than 2^64 - 1",
        })
    }
}

/// The kinds of part, declared in the order the grammar places them, which is
/// also how two parts of different kinds rank: see [`compare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Number,
    Letter,
    Suffix,
    SuffixNumber,
    Commit,
    Revision,
    End,
    /// A part the grammar does not allow where it stands. It ends the
    /// version, and ranks after every other kind, running out included.
    Invalid,
}

/// A suffix's name, declared in the order suffixes rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Suffix {
    Alpha,
    Beta,
    Pre,
    Rc,
    Cvs,
    Svn,
    Git,
    Hg,
    P,
}

impl Suffix {
    fn named(name: &[u8]) -> Option<Suffix> {
        Some(match name {
            b"alpha" => Suffix::Alpha,
            b"beta" => Suffix::Beta,
            b"pre" => Suffix::Pre,
            b"rc" => Suffix::Rc,
            b"cvs" => Suffix::Cvs,
            b"svn" => Suffix::Svn,
            b"git" => Suffix::Git,
            b"hg" => Suffix::Hg,
            b"p" => Suffix::P,
            _ => return None,
        })
    }

    /// Whether the suffix marks a pre-release, which comes before the version
    /// without it.
    fn is_pre_release(self) -> bool {
        self < Suffix::Cvs
    }
}

/// One part of a version, borrowing the text it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part<'a> {
    /// A number, the first or a dotted one: its digits as written, and their
    /// value.
    Number(&'a [u8], u64),
    Letter(u8),
    Suffix(Suffix),
    SuffixNumber(u64),
    /// A commit's hexadecimal digits.
    Commit(&'a [u8]),
    Revision(u64),
    /// The version has run out.
    End,
    /// The rest of the text is not a version.
    Invalid,
}

impl Part<'_> {
    fn kind(&self) -> Kind {
        match self {
            Part::Number(..) => Kind::Number,
            Part::Letter(_) => Kind::Letter,
            Part::Suffix(_) => Kind::Suffix,
            Part::SuffixNumber(_) => Kind::SuffixNumber,
            Part::Commit(_) => Kind::Commit,
            Part::Revision(_) => Kind::Revision,
            Part::End => Kind::End,
            Part::Invalid => Kind::Invalid,
        }
    }

    fn is_pre_release(&self) -> bool {
        matches!(self, Part::Suffix(suffix) if suffix.is_pre_release())
    }
}

/// Reads a version's parts in order, refusing one out of place.
///
/// After an error, or once the text has run out, every further part is
/// [`Part::End`].
struct Lexer<'a> {
    whole: &'a [u8],
    /// The unread text, a tail of `whole`.
    rest: &'a [u8],
    /// The kind of the part read last, or `None` before the first.
    last: Option<Kind>,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a [u8]) -> Lexer<'a> {
        Lexer {
            whole: text,
            rest: text,
            last: None,
        }
    }

    fn fail(&mut self, kind: VersionErrorKind) -> VersionError {
        let error = VersionError {
            kind,
            offset: offset_of(self.whole, self.rest),
        };
        self.rest = &[];
        self.last = Some(Kind::End);
        error
    }

    fn next(&mut self) -> Result<Part<'a>, VersionError> {
        let (first, tail) = match self.rest.split_first() {
            Some((&first, tail)) => (Some(first), tail),
            None => (None, self.rest),
        };
        let part = match (self.last, first) {
            (Some(_), None) => Part::End,
            (None, None) => return Err(self.fail(VersionErrorKind::Empty)),
            (None, Some(_)) => {
                let (digits, value) = self.digits()?;
                Part::Number(digits, value)
            }
            (Some(last), Some(character)) => match character {
                b'.' if last == Kind::Number => {
                    self.rest = tail;
                    let (digits, value) = self.digits()?;
                    Part::Number(digits, value)
                }
                b'a'..=b'z' if last == Kind::Number => {
                    self.rest = tail;
                    Part::Letter(character)
                }
                b'_' if last < Kind::Commit => {
                    self.rest = tail;
                    let (name, tail) =
                        split_run(self.rest, |character| character.is_ascii_lowercase());
                    let Some(suffix) = Suffix::named(name) else {
                        return Err(self.fail(VersionErrorKind::UnknownSuffix));
                    };
                    self.rest = tail;
                    Part::Suffix(suffix)
                }
                b'0'..=b'9' if last == Kind::Suffix => Part::SuffixNumber(self.digits()?.1),
                b'~' if last < Kind::Commit => {
                    self.rest = tail;
                    let (digits, tail) = split_run(
                        self.rest,
                        |character| matches!(character, b'0'..=b'9' | b'a'..=b'f'),
                    );
                    if digits.is_empty() {
                        return Err(self.fail(VersionErrorKind::Unexpected));
                    }
                    self.rest = tail;
                    Part::Commit(digits)
                }
                b'-' if last < Kind::Revision => {
                    let Some(tail) = self.rest.strip_prefix(b"-r") else {
                        return Err(self.fail(VersionErrorKind::Unexpected));
                    };
                    self.rest = tail;
                    Part::Revision(self.digits()?.1)
                }
                _ => return Err(self.fail(VersionErrorKind::Unexpected)),
            },
        };
        self.last = Some(part.kind());
        Ok(part)
    }

    /// Reads a run of at least one digit, and its value.
    fn digits(&mut self) -> Result<(&'a [u8], u64), VersionError> {
        let (digits, tail) = split_run(self.rest, |character| character.is_ascii_digit());
        if digits.is_empty() {
            return Err(self.fail(VersionErrorKind::Unexpected));
        }
        let value = digits.iter().try_fold(0_u64, |value, &digit| {
            value
                .checked_mul(10)?
                .checked_add(u64::from(digit.checked_sub(b'0')?))
        });
        let Some(value) = value else {
            return Err(self.fail(VersionErrorKind::TooLarge));
        };
        self.rest = tail;
        Ok((digits, value))
    }
}

/// How far [`compare`] reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Extent {
    /// Both versions in full: the ordering.
    Whole,
    /// Only as far as the second version runs.
    Prefix,
}

/// Reads `text` through to its end, refusing it where the grammar does.
fn validate(text: &[u8]) -> Result<(), VersionError> {
    let mut lexer = Lexer::new(text);
    while lexer.next()? != Part::End {}
    Ok(())
}

/// Compares two versions, either of which may be one the grammar does not
/// allow: where one goes wrong, it reads as [`Part::Invalid`].
fn compare(left: &[u8], right: &[u8], extent: Extent) -> Ordering {
    let mut left = Lexer::new(left);
    let mut right = Lexer::new(right);
    let mut first = true;
    loop {
        let (a, b) = (
            left.next().unwrap_or(Part::Invalid),
            right.next().unwrap_or(Part::Invalid),
        );
        if extent == Extent::Prefix && b == Part::End {
            return Ordering::Equal;
        }
        if a.kind() != b.kind() {
            return if a.is_pre_release() {
                Ordering::Less
            } else if b.is_pre_release() {
                Ordering::Greater
            } else {
                // The part earlier in the grammar makes the later version.
                b.kind().cmp(&a.kind())
            };
        }
        let order = match (a, b) {
            (Part::End | Part::Invalid, _) => return Ordering::Equal,
            (Part::Number(a, _), Part::Number(b, _))
                if !first && (a.starts_with(b"0") || b.starts_with(b"0")) =>
            {
                a.cmp(b)
            }
            (Part::Number(_, a), Part::Number(_, b))
            | (Part::SuffixNumber(a), Part::SuffixNumber(b))
            | (Part::Revision(a), Part::Revision(b)) => a.cmp(&b),
            (Part::Letter(a), Part::Letter(b)) => a.cmp(&b),
            (Part::Suffix(a), Part::Suffix(b)) => a.cmp(&b),
            (Part::Commit(a), Part::Commit(b)) => a.cmp(b),
            // The kinds are equal, so no other pairing arises.
            _ => Ordering::Equal,
        };
        if order.is_ne() {
            return order;
        }
        first = false;
    }
}

#[cfg(test)]
mod tests;
