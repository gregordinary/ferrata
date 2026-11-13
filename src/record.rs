//! Package records: what a repository's index, or a root's installed
//! database, states about one package.
//!
//! A record is a list of fields, one to a line, each a single byte, its
//! letter, a colon, and a value. ferrata reads the fields a resolution turns
//! on, and keeps every field as written:
//!
//! | Field | Holds | Read as |
//! | --- | --- | --- |
//! | `P:` | the package's name | [`Record::name`] |
//! | `V:` | its version | [`Record::version`] |
//! | `C:` | its identity checksum | [`Record::identity`] |
//! | `A:` | the architecture it was built for | [`Record::architecture`] |
//! | `o:` | the origin, the source package it was built from | [`Record::origin`] |
//! | `k:` | its priority among the providers of a name | [`Record::priority`] |
//! | `D:` | what it depends on, and conflicts with | [`Record::depends`] |
//! | `p:` | the names it provides | [`Record::provides`] |
//! | `i:` | the packages whose presence installs it | [`Record::install_if`] |
//!
//! The [`index`](crate::index) module sets out how a record is read.
//!
//! # Bytes
//!
//! apk reads a record as bytes, and holds no field to UTF-8 (measured with
//! apk.static 3.0.9): a byte that is not UTF-8 is a byte like any other.
//! A record keeps what it was read from byte for byte, and gives each field
//! as what apk makes of it:
//!
//! - **Names**, the package's own and those its entries name, are a
//!   [`Name`]: the bytes apk compares, which a caller reads as text with
//!   [`Name::to_str`]. apk installs a package whose name is not UTF-8 when
//!   something asks for those bytes.
//! - **Versions** are a [`Version`]. The grammar is ASCII, so a byte that is
//!   not is out of place, as any other would be: the version is not
//!   [valid](Version::is_valid), and orders as far as it reads. An entry
//!   whose version holds one does not read.
//! - **Everything else**, a description, a URL, a maintainer, the
//!   architecture, the origin, is bytes, as written ([`Record::field`]).
//!   Alpine's indexes hold text other than ASCII only there, and only UTF-8.
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;

use crate::dependency::{Dependency, DependencyError};
use crate::name::Text;
use crate::version::Version;
use crate::{Dialect, Name};

/// One package's record.
///
/// Made by the [`index`](crate::index) reader, or from its fields by
/// [`Record::from_fields`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub(crate) name: Name,
    pub(crate) version: Version,
    pub(crate) identity: Identity,
    pub(crate) architecture: Option<Box<[u8]>>,
    pub(crate) origin: Option<Box<[u8]>>,
    pub(crate) priority: u16,
    pub(crate) depends: Vec<Dependency>,
    pub(crate) provides: Vec<Provide>,
    pub(crate) install_if: Vec<Dependency>,
    /// Every field the record holds, one to a line, each line ending in a
    /// line feed.
    pub(crate) text: Box<[u8]>,
    /// Each field's letter, and where its value lies in `text`.
    pub(crate) fields: Vec<(u8, Range<usize>)>,
    pub(crate) flaws: Vec<Flaw>,
}

impl Record {
    /// Makes the record `fields` state, each a field's letter and its value,
    /// in order, as [`index::read`](crate::index::read) reads the record
    /// whose lines they are: the same record, with the same
    /// [flaws](Record::flaws), refused for the same errors. Repeated fields,
    /// letters apk does not know, and entries that do not read are what
    /// they are in an index; the [`index`](crate::index) module sets out how
    /// each is read.
    ///
    /// ```
    /// use ferrata::Dialect;
    /// use ferrata::record::{Flaw, Record};
    ///
    /// let record = Record::from_fields(
    ///     Dialect::V3_0,
    ///     [
    ///         (b'C', "Q1NH6x7ZVrTqn3VGktcofFNWa+fmQ="),
    ///         (b'P', "7zip"),
    ///         (b'V', "26.01-r0"),
    ///         (b'D', "so:libc.musl-x86_64.so.1 lib=x"),
    ///     ],
    /// )?;
    /// assert_eq!(record.name(), "7zip");
    /// assert_eq!(record.depends()[0].name(), "so:libc.musl-x86_64.so.1");
    /// assert!(matches!(record.flaws(), [Flaw::Depends(_)]));
    /// assert_eq!(
    ///     record.as_bytes(),
    ///     b"C:Q1NH6x7ZVrTqn3VGktcofFNWa+fmQ=\nP:7zip\nV:26.01-r0\n\
    ///       D:so:libc.musl-x86_64.so.1 lib=x\n",
    /// );
    /// # Ok::<(), ferrata::index::ReadError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns the [`ReadError`](crate::index::ReadError) the reader
    /// reports for the record whose lines the fields are, each field's line
    /// its place among them, counting from one: no version, no identity
    /// checksum, or one that does not read. Two more come only from here: a
    /// field whose letter or value holds a line feed, which would end its
    /// line, is [`NotAField`](crate::index::ReadErrorKind::NotAField), and
    /// fields without `P:`, which the reader passes over, are
    /// [`NoName`](crate::index::ReadErrorKind::NoName). An error found once
    /// the fields end names the last of them, or the first place where
    /// there are none.
    pub fn from_fields<V: AsRef<[u8]>>(
        dialect: Dialect,
        fields: impl IntoIterator<Item = (u8, V)>,
    ) -> Result<Record, crate::index::ReadError> {
        crate::index::record_of(dialect, fields)
    }

    /// The package's name, from `P:`, which may be empty. Where a record
    /// states it twice, the second holds, as in apk.
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The package's version, from `V:`. Where a record states it twice, the
    /// second holds, as in apk.
    ///
    /// apk loads a record whose version the grammar does not allow, so this
    /// one may not be [valid](Version::is_valid).
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// The package's identity checksum, from `C:`.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// The architecture the package was built for, from `A:`, if stated, as
    /// written.
    pub fn architecture(&self) -> Option<&[u8]> {
        self.architecture.as_deref()
    }

    /// The origin, the name of the source package the package was built
    /// from, from `o:`, if stated, as written.
    pub fn origin(&self) -> Option<&[u8]> {
        self.origin.as_deref()
    }

    /// The package's priority among the providers of a name, from `k:`: 0
    /// where the record states none.
    ///
    /// apk reads the digits the field begins with, as a number that holds at
    /// most 2^64 - 1, and keeps that number modulo 2^16: `k:65546` is 10.
    /// A field that does not begin with a digit is 0.
    pub fn priority(&self) -> u16 {
        self.priority
    }

    /// What the package depends on, and conflicts with, from `D:`, in the
    /// order stated.
    ///
    /// An entry that does not read is left out and recorded as a
    /// [`Flaw::Depends`], which makes the record one apk cannot install.
    pub fn depends(&self) -> &[Dependency] {
        &self.depends
    }

    /// The names the package provides, from `p:`, in the order stated.
    ///
    /// An entry that does not read, recorded as a [`Flaw::Provides`], ends its
    /// field: the entries before it are kept, and those after it in the same
    /// field are not.
    pub fn provides(&self) -> &[Provide] {
        &self.provides
    }

    /// The packages whose presence installs this one, from `i:`, in the order
    /// stated: all of them installed, and this package's own dependencies
    /// met, apk installs it too.
    ///
    /// An entry that does not read, recorded as a [`Flaw::InstallIf`], empties
    /// the condition, including what earlier `i:` fields stated; a later
    /// `i:` field adds to it again. An empty condition installs nothing.
    pub fn install_if(&self) -> &[Dependency] {
        &self.install_if
    }

    /// The value of the last field with `letter`, as written.
    ///
    /// ```
    /// use ferrata::Dialect;
    /// use ferrata::index;
    ///
    /// let text = b"C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\nV:1\nT:caf\xe9\n";
    /// let record = index::read(Dialect::V3_0, text).next().unwrap()?.into_record();
    /// assert_eq!(record.field(b'T'), Some(b"caf\xe9".as_slice()));
    /// assert_eq!(record.field(b'm'), None);
    /// # Ok::<(), ferrata::index::ReadError>(())
    /// ```
    pub fn field(&self, letter: u8) -> Option<&[u8]> {
        let (_, value) = self.fields.iter().rfind(|(field, _)| *field == letter)?;
        self.text.get(value.clone())
    }

    /// Every field of the record, in order: its letter and its value, as
    /// written.
    ///
    /// A record read from an installed database leaves out the lines that
    /// list the package's files.
    pub fn fields(&self) -> impl Iterator<Item = (u8, &[u8])> {
        self.fields
            .iter()
            .map(|(letter, value)| (*letter, self.text.get(value.clone()).unwrap_or_default()))
    }

    /// Every field of the record, one to a line, as read, each line ending
    /// in a line feed. For a record read from an index, that is the text of
    /// its [span](crate::index::Located::span) without its blank lines, with
    /// a line feed added where the text ended without one.
    pub fn as_bytes(&self) -> &[u8] {
        &self.text
    }

    /// What apk holds against the record, in the order it was found.
    pub fn flaws(&self) -> &[Flaw] {
        &self.flaws
    }

    /// Whether the package answers `entry`: whether it carries the entry's
    /// name or provides it, as the entry admits, by the rule the resolver
    /// follows.
    ///
    /// A package carrying the name answers at its own version, and one
    /// providing it at the version it provides it at; a name provided at no
    /// version answers an entry that states nothing but the name, or whose
    /// operator admits an earlier version
    /// ([`Dependency::admits_no_version`]). A name the package states more
    /// than once, as its own and as provided or provided twice, answers as
    /// its first mention does. An entry naming an identity (`name><Q1…`) is
    /// answered only by the package carrying the name with that identity.
    /// The `!` plays no part: for a `!` entry, this says whether the entry
    /// forbids the package.
    ///
    /// Whether apk could install the package is another question
    /// ([`Record::is_installable`]).
    ///
    /// ```
    /// use ferrata::{Dialect, index};
    ///
    /// let text = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:zlib\nV:1.3-r0\n\
    ///             p:so:libz.so.1=1.3 libz\n";
    /// let zlib = index::read(Dialect::V3_0, text).next().unwrap()?.into_record();
    /// let answers = |entry| Ok::<_, ferrata::dependency::DependencyError>(
    ///     zlib.answers(&Dialect::V3_0.dependency(entry)?),
    /// );
    /// assert!(answers("zlib<2")?);
    /// assert!(answers("so:libz.so.1>=1.2")?);
    /// assert!(!answers("so:libz.so.1>=2")?);
    /// assert!(answers("libz")?);
    /// assert!(answers("libz<1")?);
    /// assert!(!answers("libz>=1")?);
    /// assert!(answers("!zlib")?);
    /// assert!(answers("zlib><Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=")?);
    /// assert!(!answers("libz><Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=")?);
    /// assert!(!answers("musl")?);
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    pub fn answers(&self, entry: &Dependency) -> bool {
        self.answers_on(entry.name(), Some(entry))
    }

    /// Whether the package is a candidate for `name` that `entry` admits,
    /// or with no entry, a candidate for `name` at all: what
    /// [`Record::answers`] asks of an entry, and
    /// [`Requirement::answered_by`](crate::resolve::Requirement::answered_by)
    /// of a requirement, which may have none.
    pub(crate) fn answers_on(&self, name: &Name, entry: Option<&Dependency>) -> bool {
        let carried = self.name == *name;
        self.candidacies()
            .find(|&(candidate, _)| candidate == name)
            .is_some_and(|(_, version)| self.admitted(entry, version, carried))
    }

    /// Each name the package is a candidate for, and the version it is one
    /// at: its own name at its own version, then each name it provides, at
    /// the version it provides it at or at none, in the order stated. A
    /// name stated twice is a candidate at its first mention.
    pub(crate) fn candidacies(&self) -> impl Iterator<Item = (&Name, Option<&Version>)> {
        core::iter::once((&self.name, Some(&self.version))).chain(
            self.provides
                .iter()
                .map(|provide| (provide.name(), provide.version())),
        )
    }

    /// Whether `entry` admits the package as a candidate for the entry's
    /// name at `version`, its own name where `carried`: the rule
    /// [`Record::answers`] and the resolver share. An entry naming an
    /// identity admits the package carrying the name with that identity;
    /// any other admits a candidate at a version its constraint admits, and
    /// one at no version where it admits an earlier version. No entry, as a
    /// requirement every candidate for another name shares has none, admits
    /// every candidate.
    pub(crate) fn admitted(
        &self,
        entry: Option<&Dependency>,
        version: Option<&Version>,
        carried: bool,
    ) -> bool {
        let Some(entry) = entry else {
            return true;
        };
        if let Some(identity) = entry.identity() {
            return carried && Identity::read(identity).as_ref() == Some(&self.identity);
        }
        match version {
            Some(version) => entry.admits(version),
            None => entry.admits_no_version(),
        }
    }

    /// Whether apk can install the package: whether none of its
    /// [flaws](Record::flaws) forbids it, and it does not
    /// [conflict with itself](Record::conflicts_with_itself).
    pub fn is_installable(&self) -> bool {
        self.is_readable() && !self.conflicts_with_itself()
    }

    /// Whether none of the record's flaws forbids apk to install it: apk
    /// takes it as a package, which it may then find conflicts with itself.
    pub(crate) fn is_readable(&self) -> bool {
        !self.flaws.iter().any(|flaw| match flaw {
            Flaw::UnknownField(letter) => !letter.is_ascii_lowercase(),
            Flaw::Depends(_) => true,
            Flaw::Provides | Flaw::InstallIf(_) => false,
        })
    }

    /// Whether the package conflicts with itself, which apk never installs:
    /// it provides its own name, in any form, or provides another name
    /// twice, at a version at least once. A name provided twice at no
    /// version is no conflict (the behavior catalog's
    /// `providers/a-package-providing-a-name-twice-conflicts-with-itself`).
    /// apk still chooses such a package as it would any other, and then
    /// refuses the request
    /// (`providers/a-package-conflicting-with-itself-is-still-chosen`).
    ///
    /// ```
    /// use ferrata::Dialect;
    /// use ferrata::index::{self, Located};
    ///
    /// let text = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:a\nV:1\np:x x=1\n";
    /// let record = index::read(Dialect::V3_0, text).next().unwrap()?.into_record();
    /// assert!(record.conflicts_with_itself());
    /// assert!(!record.is_installable());
    /// # Ok::<(), ferrata::index::ReadError>(())
    /// ```
    pub fn conflicts_with_itself(&self) -> bool {
        // Each name provided so far, and whether at a version.
        let mut provided: BTreeMap<&Name, bool> = BTreeMap::new();
        self.provides.iter().any(|provide| {
            let versioned = provide.version().is_some();
            provide.name() == &self.name
                || match provided.get_mut(provide.name()) {
                    Some(before) => *before || versioned,
                    None => {
                        provided.insert(provide.name(), versioned);
                        false
                    }
                }
        })
    }
}

impl fmt::Display for Record {
    /// Writes the record's fields, one to a line, as they were read, each
    /// line ending in a line feed: its [bytes](Record::as_bytes), written as
    /// [`Name`] writes a name: UTF-8 as it is, each byte that is not UTF-8 as
    /// `\xNN`, and a backslash as `\\`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&Text(&self.text), f)
    }
}

/// What apk holds against a record: a part of it apk reads and cannot use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Flaw {
    /// A field whose letter apk does not know. apk knows the lowercase
    /// letters `a`, `c`, `f`, `g`, `i`, `k`, `m`, `o`, `p`, `q`, `r`, `s` and
    /// `t`, and the uppercase letters `A`, `C`, `D`, `F`, `I`, `L`, `M`, `P`,
    /// `R`, `S`, `T`, `U`, `V` and `Z`. It installs a package with another
    /// lowercase letter, warning that it might not function properly, and
    /// cannot install one with another uppercase letter, a digit, or any
    /// other byte, one that is not ASCII included.
    UnknownField(u8),
    /// A `D:` entry that does not read. apk cannot install the package.
    Depends(DependencyError),
    /// A `p:` entry that does not read: an operator with no version after
    /// it. Neither it nor the entries after it in its field are provided.
    Provides,
    /// An `i:` entry that does not read, which empties the package's
    /// `install_if` as far as it was read.
    InstallIf(DependencyError),
}

impl fmt::Display for Flaw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Flaw::UnknownField(letter) => {
                write!(f, "an unknown field '{}'", Text(&[*letter]))
            }
            Flaw::Depends(error) => write!(f, "a dependency that does not read: {error}"),
            Flaw::Provides => f.write_str("a provided name with an operator and no version"),
            Flaw::InstallIf(error) => write!(f, "an install_if entry that does not read: {error}"),
        }
    }
}

/// A name a package provides, and the version it provides it at, if any.
///
/// A name provided at a version answers a dependency on it as a package of
/// that name and version would; one provided at no version answers a
/// dependency that states no version, or whose operator admits an earlier
/// version ([`Dependency::admits_no_version`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provide {
    name: Name,
    version: Option<Version>,
}

impl Provide {
    /// Reads one `p:` entry: a name, and optionally an operator and a
    /// version.
    ///
    /// apk takes the text after the operator as the version whatever the
    /// operator, so `x>=1.0` provides `x` at 1.0, and keeps a version the
    /// grammar does not allow. An operator with nothing after it does not
    /// read.
    pub(crate) fn read(dialect: Dialect, text: &[u8]) -> Option<Provide> {
        let operator = |byte: &u8| matches!(byte, b'<' | b'>' | b'=' | b'~');
        let at = text.iter().position(operator).unwrap_or(text.len());
        let (name, rest) = text.split_at_checked(at)?;
        let skipped = rest.iter().take_while(|byte| operator(byte)).count();
        let version = rest.get(skipped..).unwrap_or_default();
        let version = match (rest.is_empty(), version.is_empty()) {
            (true, _) => None,
            (false, true) => return None,
            (false, false) => Some(Version::read(dialect, version)),
        };
        Some(Provide {
            name: name.into(),
            version,
        })
    }

    /// The name provided.
    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The version the name is provided at, if any.
    pub fn version(&self) -> Option<&Version> {
        self.version.as_ref()
    }
}

impl fmt::Display for Provide {
    /// Writes `name`, or `name=version`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        match &self.version {
            Some(version) => write!(f, "={version}"),
            None => Ok(()),
        }
    }
}

/// A package's identity checksum: the digest apk names one build of a
/// package by.
///
/// It alone tells packages apart, as apk tells them apart: two builds of
/// one version are two packages, an installed package is matched to a
/// repository's record by it, and a record with the identity of a record
/// read before is that record's package again, whatever its name (see
/// [`Resolver::repository`](crate::resolve::Resolver::repository)).
/// `name><identity` requests the one package it names. It is written `Q1` and the base64 of a SHA-1
/// digest, or `Q2` and the base64 of a SHA-256 digest; identities compare
/// by their digests.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identity {
    digest: [u8; 32],
    sha256: bool,
}

impl Identity {
    /// Reads an identity as apk reads a `C:` field.
    ///
    /// The digest must be base64 of exactly its length, padding included:
    /// 28 characters after `Q1`, 44 after `Q2`. Anything after it is passed
    /// over, as apk passes over it.
    ///
    /// ```
    /// use ferrata::record::Identity;
    ///
    /// let identity = Identity::read("Q1NH6x7ZVrTqn3VGktcofFNWa+fmQ=").unwrap();
    /// assert_eq!(identity.digest().len(), 20);
    /// assert_eq!(identity.to_string(), "Q1NH6x7ZVrTqn3VGktcofFNWa+fmQ=");
    /// assert!(Identity::read("Q1NH6x7ZVrTqn3VGktcofFNWa+fmQ").is_none());
    /// ```
    pub fn read(text: impl AsRef<[u8]>) -> Option<Identity> {
        let text = text.as_ref();
        let (sha256, encoded, length) = match text.strip_prefix(b"Q1") {
            Some(encoded) => (false, encoded, 20),
            None => (true, text.strip_prefix(b"Q2")?, 32),
        };
        let mut digest = [0; 32];
        base64_decode(encoded, digest.get_mut(..length)?)?;
        Some(Identity { digest, sha256 })
    }

    /// The digest: 20 bytes of SHA-1, or 32 of SHA-256.
    pub fn digest(&self) -> &[u8] {
        let length = if self.sha256 { 32 } else { 20 };
        self.digest.get(..length).unwrap_or_default()
    }
}

impl fmt::Display for Identity {
    /// Writes the identity as apk writes it: `Q1` or `Q2`, and the digest in
    /// base64 with its padding.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.sha256 { "Q2" } else { "Q1" })?;
        f.write_str(&base64_encode(self.digest()))
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Identity({self})")
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_value(character: u8) -> Option<u32> {
    let at = BASE64.iter().position(|&known| known == character)?;
    u32::try_from(at).ok()
}

/// Decodes the base64 at the start of `encoded` into all of `out`, whose
/// length leaves two bytes over a multiple of three, as both digest lengths
/// do: whole groups of four characters, then three and the padding.
fn base64_decode(encoded: &[u8], out: &mut [u8]) -> Option<()> {
    let (whole, last) = out.as_chunks_mut::<3>();
    let [last_a, last_b] = last else {
        return None;
    };
    let (groups, _) = encoded.as_chunks::<4>();
    for (bytes, group) in whole.iter_mut().zip(groups) {
        let mut value = 0;
        for &character in group {
            value = (value << 6) | base64_value(character)?;
        }
        *bytes = value.to_be_bytes().get(1..)?.try_into().ok()?;
    }
    let [a, b, c, b'='] = groups.get(whole.len())? else {
        return None;
    };
    let value = (base64_value(*a)? << 18) | (base64_value(*b)? << 12) | (base64_value(*c)? << 6);
    let [_, high, low, _] = value.to_be_bytes();
    (*last_a, *last_b) = (high, low);
    Some(())
}

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut group = [0; 4];
        for (slot, &byte) in group.iter_mut().skip(1).zip(chunk) {
            *slot = byte;
        }
        let value = u32::from_be_bytes(group);
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            let character = if index <= chunk.len() {
                let at = usize::try_from((value >> shift) & 0x3f).unwrap_or_default();
                BASE64.get(at).copied().unwrap_or(b'=')
            } else {
                b'='
            };
            out.push(char::from(character));
        }
    }
    out
}

#[cfg(test)]
mod tests;
