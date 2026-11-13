//! Reading package records from an `APKINDEX`, or from an installed
//! database, as apk reads them.
//!
//! Both are lines of bytes: records separated by blank lines, each a list
//! of fields one to a line, every line a single byte (the field's letter),
//! a colon, and a value. An installed database (`lib/apk/db/installed`)
//! adds lines listing each package's files, which [`read_installed`] passes
//! over.
//!
//! The readers take bytes, as apk reads them: neither holds any line to
//! UTF-8. A file line holds a path, which may be any bytes, and a package
//! field that is not UTF-8 is read as the [`record`](crate::record) module
//! describes, as apk reads it. A caller holding the text as a `str` passes it
//! as it is.
//!
//! The text must already be authenticated: this module reads what it is
//! given, and checks no signature.
//!
//! # What apk makes of a record
//!
//! The reader follows apk 3.0, including where its reading is lenient or
//! unexpected:
//!
//! - A line is split at nothing but its line feed. A carriage return before
//!   it is part of the value.
//! - A line whose letter is a byte apk does not know as one, a byte that is
//!   not ASCII included, is a field all the same (a [`Flaw`]). A line whose
//!   second byte is not a colon is not a field: a letter that is not ASCII
//!   but UTF-8, such as `é:`, is two bytes, and is not one.
//! - A repeated `P:`, `V:`, `C:`, `A:` or `k:` field replaces the one before
//!   it, and ferrata keeps the last `o:` likewise. A repeated `D:`, `p:` or
//!   `i:` field adds to the one before it.
//! - In `p:`, an entry that does not read ends its field: the entries after
//!   it in the same field are not provided. In `i:`, it empties the
//!   condition, including what earlier `i:` fields stated; a later `i:` field
//!   adds to it again.
//! - In `D:`, `p:` and `i:`, entries are separated by spaces, any number of
//!   them, and by nothing else: a tab is part of a name. A value that begins
//!   with a space begins with an empty entry, a dependency on a name nothing
//!   provides.
//! - A record ends at a blank line only once it has a `P:` field, an empty
//!   one included. Before then, its fields carry into the record after the
//!   blank line, and at the end of the text they are dropped.
//! - The identity checksum is judged when the record ends, by its last
//!   `C:`.
//! - A version outside the grammar is kept, and ordered as the
//!   [`version`](crate::version) module describes.
//! - What apk reads and cannot use is a [`Flaw`] of the record, not an
//!   error.
//!
//! What apk cannot read at all is an error: a line that is not a field, and
//! a record without a version or without an identity checksum it can
//! decode. apk stops reading an index there, keeping the records before it,
//! and so does the reader: after an error the iterator ends. A caller that
//! wants apk's answer keeps what came before; one that wants the whole index
//! or nothing refuses it.
//!
//! A caller holding a record's fields rather than its text makes the record
//! with [`Record::from_fields`], which reads them by the same rules.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;
use core::iter::FusedIterator;
use core::ops::Range;

use crate::dependency::Dependency;
use crate::record::{Flaw, Identity, Provide, Record};
use crate::version::Version;
use crate::{Dialect, Name, offset_of};

/// Reads the records of an `APKINDEX`, in the order it states them, from
/// its bytes, or from a `str` holding them.
///
/// ```
/// use ferrata::Dialect;
///
/// let text = "C:Q1NH6x7ZVrTqn3VGktcofFNWa+fmQ=\nP:7zip\nV:26.01-r0\nA:x86_64\n\
///             D:so:libc.musl-x86_64.so.1\np:cmd:7z=26.01-r0\n\n";
/// let records = ferrata::index::read(Dialect::V3_0, text)
///     .collect::<Result<Vec<_>, _>>()?;
/// let record = records[0].record();
/// assert_eq!(record.name(), "7zip");
/// assert_eq!(record.depends()[0].name(), "so:libc.musl-x86_64.so.1");
/// assert_eq!(records[0].span(), 0..text.len() - 1);
/// # Ok::<(), ferrata::index::ReadError>(())
/// ```
pub fn read(dialect: Dialect, text: &(impl AsRef<[u8]> + ?Sized)) -> Records<'_> {
    Records::new(dialect, text.as_ref(), false)
}

/// Reads the records of an installed database, `lib/apk/db/installed`, in
/// the order it states them, passing over the lines that list each
/// package's files (`F:`, `M:`, `R:`, `a:` and `Z:`).
///
/// A record's [span](Located::span) covers its file lines too, so a caller
/// that carries the database's file lines through unchanged can split it
/// where the reader does. A path in a file line may be any bytes.
///
/// ```
/// use ferrata::Dialect;
///
/// let first = b"C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\nV:1\nF:usr/bin\nR:caf\xe9\n";
/// let database = [&first[..], b"\nC:Q1BBBBBBBBBBBBBBBBBBBBBBBBBBB=\nP:b\nV:1\n"].concat();
/// let records = ferrata::index::read_installed(Dialect::V3_0, &database)
///     .collect::<Result<Vec<_>, _>>()?;
/// assert_eq!(records[0].record().fields().count(), 3);
/// assert_eq!(&database[records[0].span()], first);
/// # Ok::<(), ferrata::index::ReadError>(())
/// ```
pub fn read_installed(dialect: Dialect, text: &(impl AsRef<[u8]> + ?Sized)) -> Records<'_> {
    Records::new(dialect, text.as_ref(), true)
}

/// The records of an index or an installed database, read one at a time.
///
/// Made by [`read`] and [`read_installed`]. Each item is a record and where
/// it was read from, or the error that ends the reading. Once it has given
/// an error, or `None`, it gives `None` for good.
#[derive(Debug, Clone)]
pub struct Records<'a> {
    dialect: Dialect,
    text: &'a [u8],
    /// The unread text, a tail of `text`.
    rest: &'a [u8],
    /// The number of the next line, counting from one.
    line: usize,
    installed: bool,
    done: bool,
}

impl<'a> Records<'a> {
    fn new(dialect: Dialect, text: &'a [u8], installed: bool) -> Records<'a> {
        Records {
            dialect,
            text,
            rest: text,
            line: 1,
            installed,
            done: false,
        }
    }

    /// Takes the next line, without its line feed, or `None` at the end.
    fn take_line(&mut self) -> Option<&'a [u8]> {
        if self.rest.is_empty() {
            return None;
        }
        let (line, rest) = match self.rest.iter().position(|&byte| byte == b'\n') {
            Some(at) => (
                self.rest.get(..at).unwrap_or_default(),
                self.rest.get(at.saturating_add(1)..).unwrap_or_default(),
            ),
            None => (self.rest, &[][..]),
        };
        self.rest = rest;
        self.line = self.line.saturating_add(1);
        Some(line)
    }

    fn read_record(&mut self) -> Option<Result<Located, ReadError>> {
        // Blank lines before a record separate it from the one before.
        while self.rest.starts_with(b"\n") {
            self.take_line();
        }
        if self.rest.is_empty() {
            return None;
        }
        let start = offset_of(self.text, self.rest);
        let first_line = self.line;
        let mut builder = Builder::new(self.dialect);
        let mut end = start;
        let mut last_line = first_line;
        while let Some(line) = self.take_line() {
            if line.is_empty() {
                // A blank line ends a record only once it has a name.
                if builder.name.is_some() {
                    break;
                }
                continue;
            }
            last_line = self.line.saturating_sub(1);
            let fail = |kind| ReadError {
                kind,
                line: last_line,
                record_line: first_line,
            };
            let Some((&letter, value)) = line.split_first() else {
                return Some(Err(fail(ReadErrorKind::NotAField)));
            };
            let Some(value) = value.strip_prefix(b":") else {
                return Some(Err(fail(ReadErrorKind::NotAField)));
            };
            end = offset_of(self.text, self.rest);
            if self.installed && matches!(letter, b'F' | b'M' | b'R' | b'a' | b'Z') {
                continue;
            }
            builder.field(letter, value, last_line);
        }
        match builder.finish() {
            // Fields with no name to end them are dropped, as apk drops them.
            Err((ReadErrorKind::NoName, _)) => None,
            finished => Some(
                finished
                    .map(|record| Located {
                        record,
                        span: start..end,
                        line: first_line,
                    })
                    .map_err(|(kind, line)| ReadError {
                        kind,
                        line: line.unwrap_or(last_line),
                        record_line: first_line,
                    }),
            ),
        }
    }
}

/// A record made of `fields`, as [`read`] reads a record whose lines they
/// are: what [`Record::from_fields`] does.
pub(crate) fn record_of<V: AsRef<[u8]>>(
    dialect: Dialect,
    fields: impl IntoIterator<Item = (u8, V)>,
) -> Result<Record, ReadError> {
    let fail = |kind, line| ReadError {
        kind,
        line,
        record_line: 1,
    };
    let mut builder = Builder::new(dialect);
    let mut line: usize = 0;
    for (letter, value) in fields {
        line = line.saturating_add(1);
        let value = value.as_ref();
        // A line feed would end the field's line, which no field of a
        // record's text can hold.
        if letter == b'\n' || value.contains(&b'\n') {
            return Err(fail(ReadErrorKind::NotAField, line));
        }
        builder.field(letter, value, line);
    }
    builder
        .finish()
        .map_err(|(kind, at)| fail(kind, at.unwrap_or(line.max(1))))
}

impl Iterator for Records<'_> {
    type Item = Result<Located, ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let item = self.read_record();
        if !matches!(item, Some(Ok(_))) {
            self.done = true;
        }
        item
    }
}

impl FusedIterator for Records<'_> {}

/// A record, and where in the text it was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    record: Record,
    span: Range<usize>,
    line: usize,
}

impl Located {
    /// The record.
    pub fn record(&self) -> &Record {
        &self.record
    }

    /// The record, taken from where it was read.
    pub fn into_record(self) -> Record {
        self.record
    }

    /// The bytes of the text the record was read from: its first line
    /// through its last, with that line's line feed where it has one. An
    /// installed record's file lines are within it, and so are the fields
    /// before it that had no name to end them, with the blank lines
    /// between.
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// The number of the record's first line, counting from one.
    pub fn line(&self) -> usize {
        self.line
    }
}

/// Text the reader, like apk, cannot read on from: what was wrong, and
/// where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadError {
    kind: ReadErrorKind,
    line: usize,
    record_line: usize,
}

impl ReadError {
    /// What was wrong.
    pub fn kind(&self) -> ReadErrorKind {
        self.kind
    }

    /// The number of the line the failure was found on, counting from one.
    /// A record missing a field fails on its last line, and one whose
    /// identity checksum does not read on the line of its last `C:`.
    pub fn line(&self) -> usize {
        self.line
    }

    /// The number of the first line of the record the failure was found in.
    pub fn record_line(&self) -> usize {
        self.record_line
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}, in the record from line {}: {}",
            self.line, self.record_line, self.kind
        )
    }
}

impl core::error::Error for ReadError {}

/// What the reader could not read, as a [`ReadError`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReadErrorKind {
    /// A line that is not a single byte, a colon, and a value; or a field
    /// given to [`Record::from_fields`] whose letter or value holds a line
    /// feed, which would end its line.
    NotAField,
    /// A record without `V:`.
    NoVersion,
    /// A record without `C:`.
    NoIdentity,
    /// A `C:` field that is not an identity checksum.
    Identity,
    /// A record without `P:`. The reader passes over fields no `P:` ends,
    /// as apk does, so only [`Record::from_fields`] reports it.
    NoName,
}

impl fmt::Display for ReadErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ReadErrorKind::NotAField => "a line that is not a field",
            ReadErrorKind::NoVersion => "no version",
            ReadErrorKind::NoIdentity => "no identity checksum",
            ReadErrorKind::Identity => "an identity checksum that does not read",
            ReadErrorKind::NoName => "no name",
        })
    }
}

/// A record as its fields are read.
struct Builder {
    dialect: Dialect,
    name: Option<Name>,
    version: Option<Version>,
    /// The last `C:`, and the line it is on: judged when the record ends.
    identity: Option<(Vec<u8>, usize)>,
    architecture: Option<Box<[u8]>>,
    origin: Option<Box<[u8]>>,
    priority: u16,
    depends: Vec<Dependency>,
    provides: Vec<Provide>,
    install_if: Vec<Dependency>,
    text: Vec<u8>,
    fields: Vec<(u8, Range<usize>)>,
    flaws: Vec<Flaw>,
}

impl Builder {
    fn new(dialect: Dialect) -> Builder {
        Builder {
            dialect,
            name: None,
            version: None,
            identity: None,
            architecture: None,
            origin: None,
            priority: 0,
            depends: Vec::new(),
            provides: Vec::new(),
            install_if: Vec::new(),
            text: Vec::new(),
            fields: Vec::new(),
            flaws: Vec::new(),
        }
    }

    fn field(&mut self, letter: u8, value: &[u8], line: usize) {
        self.text.push(letter);
        self.text.push(b':');
        let start = self.text.len();
        self.text.extend_from_slice(value);
        self.fields.push((letter, start..self.text.len()));
        self.text.push(b'\n');

        let dialect = self.dialect;
        match letter {
            b'P' => self.name = Some(value.into()),
            b'V' => self.version = Some(Version::read(dialect, value)),
            b'C' => self.identity = Some((value.into(), line)),
            b'A' => self.architecture = Some(value.into()),
            b'o' => self.origin = Some(value.into()),
            b'k' => self.priority = priority(value),
            b'D' => {
                for entry in entries(value) {
                    match entry_dependency(dialect, entry) {
                        Ok(dependency) => self.depends.push(dependency),
                        Err(error) => self.flaws.push(Flaw::Depends(error)),
                    }
                }
            }
            b'p' => {
                for entry in entries(value) {
                    let Some(provide) = Provide::read(dialect, entry) else {
                        self.flaws.push(Flaw::Provides);
                        break;
                    };
                    self.provides.push(provide);
                }
            }
            b'i' => {
                for entry in entries(value) {
                    match entry_dependency(dialect, entry) {
                        Ok(dependency) => self.install_if.push(dependency),
                        Err(error) => {
                            self.install_if.clear();
                            self.flaws.push(Flaw::InstallIf(error));
                            break;
                        }
                    }
                }
            }
            b'a' | b'c' | b'f' | b'g' | b'm' | b'q' | b'r' | b's' | b't' => {}
            b'F' | b'I' | b'L' | b'M' | b'R' | b'S' | b'T' | b'U' | b'Z' => {}
            _ => self.flaws.push(Flaw::UnknownField(letter)),
        }
    }

    /// The record, or what is missing from it and, for an identity that
    /// does not read, the line of its `C:`.
    fn finish(self) -> Result<Record, (ReadErrorKind, Option<usize>)> {
        let name = self.name.ok_or((ReadErrorKind::NoName, None))?;
        let version = self.version.ok_or((ReadErrorKind::NoVersion, None))?;
        let (identity, line) = self.identity.ok_or((ReadErrorKind::NoIdentity, None))?;
        let identity = Identity::read(&identity).ok_or((ReadErrorKind::Identity, Some(line)))?;
        Ok(Record {
            name,
            version,
            identity,
            architecture: self.architecture,
            origin: self.origin,
            priority: self.priority,
            depends: self.depends,
            provides: self.provides,
            install_if: self.install_if,
            text: self.text.into(),
            fields: self.fields,
            flaws: self.flaws,
        })
    }
}

/// The entries of a list field, as apk splits it: at runs of spaces, the
/// first entry starting at the first byte even where that is a space.
fn entries(value: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = Some(value).filter(|value| !value.is_empty());
    core::iter::from_fn(move || {
        let text = rest?;
        let (entry, after) = match text.iter().position(|&byte| byte == b' ') {
            Some(at) => (
                text.get(..at).unwrap_or_default(),
                text.get(at..).unwrap_or_default(),
            ),
            None => (text, &[][..]),
        };
        let spaces = after.iter().take_while(|&&byte| byte == b' ').count();
        let after = after.get(spaces..).unwrap_or_default();
        rest = Some(after).filter(|after| !after.is_empty());
        Some(entry)
    })
}

/// Reads one `D:` or `i:` entry. An empty one, which apk reads where a list
/// begins with a space, is a dependency on the empty name.
fn entry_dependency(
    dialect: Dialect,
    entry: &[u8],
) -> Result<Dependency, crate::dependency::DependencyError> {
    if entry.is_empty() {
        return Ok(Dependency::on_empty_name(dialect));
    }
    dialect.dependency(entry)
}

/// Reads `k:` as apk does: the digits it begins with, saturating at 2^64 - 1,
/// kept modulo 2^16.
fn priority(value: &[u8]) -> u16 {
    let number = value
        .iter()
        .map_while(|byte| byte.is_ascii_digit().then(|| byte.saturating_sub(b'0')))
        .fold(0_u64, |number, digit| {
            number.saturating_mul(10).saturating_add(u64::from(digit))
        });
    u16::try_from(number & 0xffff).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests;
