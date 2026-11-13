//! The apk series whose rules a caller asks for, and the one a release's
//! apk-tools follows ([`Dialect::for_release`]).

use core::cmp::Ordering;
use core::fmt;

use crate::dependency::{Dependency, DependencyError};
use crate::record::Record;
use crate::version::{Version, VersionError};

/// A series of apk-tools, whose rules ferrata follows.
///
/// apk's version syntax, its dependency matching, and the choices its solver
/// makes have each changed between series, so every question ferrata answers
/// is asked of one. A caller names the series of the apk that carries out the
/// transaction, which is usually the one its Alpine release ships.
///
/// Each dialect follows the newest point release of its series, and
/// [`measured_against`](Dialect::measured_against) names the release its
/// rules were last measured against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Dialect {
    /// apk-tools 3.0, which Alpine ships from v3.23.
    V3_0,
}

impl Dialect {
    /// Every dialect, the oldest series first.
    const ALL: [Dialect; 1] = [Dialect::V3_0];

    /// The series of apk-tools whose rules this dialect is, as a version
    /// every release of the series begins with.
    const fn series(self) -> &'static str {
        match self {
            Dialect::V3_0 => "3.0",
        }
    }

    /// The dialect that answers for a release, chosen by the apk-tools it
    /// ships: the `apk-tools` package among `records`, those of the
    /// release's `main` repository, at its highest version where there are
    /// several.
    ///
    /// Every release of a dialect's series is answered by it, a release
    /// other than the one its rules were measured against included, which
    /// [`Selection::is_measured`] reports so that a caller can warn. A
    /// series no dialect follows, such as an older one or a pre-release of
    /// a newer one, is refused rather than answered by the nearest: a
    /// caller that wants it answered by a dialect's rules names that dialect
    /// itself.
    ///
    /// The records may be read under any dialect: a record's name, and the
    /// version of an `apk-tools` release, read the same under each. The
    /// highest `apk-tools` among them decides, so a caller passes those of
    /// the release's `main` repository where it can tell them apart: another
    /// repository offering a later or a pre-release `apk-tools` would decide
    /// otherwise.
    ///
    /// ```
    /// use ferrata::{Dialect, index};
    ///
    /// let main = "C:Q1AAAAAAAAAAAAAAAAAAAAAAAAAAA=\nP:apk-tools\nV:3.0.9-r0\n";
    /// let records: Vec<_> = index::read(Dialect::V3_0, main)
    ///     .map(|located| located.map(index::Located::into_record))
    ///     .collect::<Result<_, _>>()?;
    /// let selection = Dialect::for_release(&records)?;
    /// assert_eq!(selection.dialect(), Dialect::V3_0);
    /// assert!(selection.is_measured());
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`DialectError`] where `records` hold no `apk-tools`
    /// package, or where no dialect follows the version of the one they
    /// hold.
    pub fn for_release<'r>(
        records: impl IntoIterator<Item = &'r Record>,
    ) -> Result<Selection, DialectError> {
        let apk_tools = records
            .into_iter()
            .filter(|record| record.name() == "apk-tools")
            .map(Record::version)
            .max()
            .ok_or(DialectError {
                kind: DialectErrorKind::NoApkTools,
                apk_tools: None,
            })?;
        let follows = |dialect: &Dialect| {
            apk_tools.is_valid()
                && apk_tools
                    .dialect()
                    .version(dialect.series())
                    .is_ok_and(|series| apk_tools.compare_prefix(&series) == Ordering::Equal)
        };
        match Dialect::ALL.into_iter().find(follows) {
            Some(dialect) => Ok(Selection {
                dialect,
                apk_tools: apk_tools.clone(),
            }),
            None => Err(DialectError {
                kind: DialectErrorKind::NoDialect,
                apk_tools: Some(apk_tools.clone()),
            }),
        }
    }

    /// The apk-tools release this dialect's rules were last measured against.
    ///
    /// ```
    /// assert_eq!(ferrata::Dialect::V3_0.measured_against(), "3.0.9");
    /// ```
    pub const fn measured_against(self) -> &'static str {
        match self {
            Dialect::V3_0 => "3.0.9",
        }
    }

    /// Reads a package version: text, or the bytes apk is given.
    ///
    /// ```
    /// use ferrata::Dialect;
    ///
    /// let older = Dialect::V3_0.version("1.0_rc1-r0")?;
    /// let newer = Dialect::V3_0.version("1.0-r0")?;
    /// assert!(older < newer);
    /// # Ok::<(), ferrata::version::VersionError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`VersionError`] for text this dialect does not read as a
    /// version. The [`version`](crate::version) module sets out what it reads.
    pub fn version(self, text: impl AsRef<[u8]>) -> Result<Version, VersionError> {
        Version::parse(self, text.as_ref())
    }

    /// Reads a package version as apk reads a record's: any text or bytes,
    /// those the grammar does not allow included, which are kept and ordered
    /// as far as they can be read (see the [`version`](crate::version)
    /// module). [`Version::is_valid`] says whether the grammar allows it.
    ///
    /// This is the version an [`index`](crate::index) record carries, for a
    /// version that comes from elsewhere, such as a lock file or another
    /// tool's output.
    ///
    /// ```
    /// use ferrata::Dialect;
    ///
    /// let odd = Dialect::V3_0.recorded_version("1.0b1-r0");
    /// assert!(!odd.is_valid());
    /// assert!(Dialect::V3_0.version("1.0b1-r0").is_err());
    /// assert!(odd > Dialect::V3_0.version("1.0-r0")?);
    /// # Ok::<(), ferrata::version::VersionError>(())
    /// ```
    pub fn recorded_version(self, text: impl AsRef<[u8]>) -> Version {
        Version::read(self, text.as_ref())
    }

    /// Reads one dependency entry, as a package's dependencies, a world file,
    /// or an `apk add` request states it: text, or the bytes apk reads, which
    /// need not be UTF-8 (see [`Name`](crate::Name)).
    ///
    /// ```
    /// use ferrata::Dialect;
    ///
    /// let entry = Dialect::V3_0.dependency("musl>=1.2")?;
    /// assert_eq!(entry.name(), "musl");
    /// assert!(entry.admits(&Dialect::V3_0.version("1.2.5-r10")?));
    /// # Ok::<(), Box<dyn core::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`DependencyError`] for text this dialect does not read as a
    /// dependency. The [`dependency`](crate::dependency) module sets out what
    /// it reads.
    pub fn dependency(self, text: impl AsRef<[u8]>) -> Result<Dependency, DependencyError> {
        Dependency::parse(self, text.as_ref())
    }
}

/// The dialect a release's apk-tools follows, as [`Dialect::for_release`]
/// finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    dialect: Dialect,
    apk_tools: Version,
}

impl Selection {
    /// The dialect that answers for the release.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// The version of the apk-tools the release ships.
    pub fn apk_tools(&self) -> &Version {
        &self.apk_tools
    }

    /// Whether the release ships the apk-tools the dialect's rules were
    /// measured against ([`Dialect::measured_against`]). A rebuild of that
    /// release by Alpine, another `-r` revision, counts as it; any other
    /// release of the series does not.
    pub fn is_measured(&self) -> bool {
        upstream(self.apk_tools.as_bytes()) == self.dialect.measured_against().as_bytes()
    }
}

/// `version` without its trailing `-r` revision, where it has one.
fn upstream(version: &[u8]) -> &[u8] {
    let Some(at) = version.iter().rposition(|&byte| byte == b'-') else {
        return version;
    };
    let (head, tail) = version.split_at(at);
    match tail.strip_prefix(b"-r") {
        Some(digits) if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) => head,
        _ => version,
    }
}

/// Why no dialect answers for a release ([`Dialect::for_release`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectError {
    kind: DialectErrorKind,
    apk_tools: Option<Version>,
}

impl DialectError {
    /// What was wrong.
    pub fn kind(&self) -> DialectErrorKind {
        self.kind
    }

    /// The version of the apk-tools the release ships, where it ships one.
    pub fn apk_tools(&self) -> Option<&Version> {
        self.apk_tools.as_ref()
    }
}

impl fmt::Display for DialectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.apk_tools {
            Some(version) => write!(f, "{} {version}", self.kind),
            None => write!(f, "{}", self.kind),
        }
    }
}

impl core::error::Error for DialectError {}

/// What kept [`Dialect::for_release`] from choosing a dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DialectErrorKind {
    /// The records hold no `apk-tools` package.
    NoApkTools,
    /// No dialect follows the series of the release's apk-tools.
    NoDialect,
}

impl fmt::Display for DialectErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DialectErrorKind::NoApkTools => "no apk-tools package among the release's records",
            DialectErrorKind::NoDialect => "no dialect follows apk-tools",
        })
    }
}

#[cfg(test)]
mod tests;
