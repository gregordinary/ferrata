//! What the harness takes from Alpine's mirrors: apk.static, the keys it
//! verifies indexes with, and the indexes of the repositories a sweep
//! resolves over.
//!
//! apk.static is the oracle every recorded answer comes from. It is the
//! `apk-tools-static` binary package of a named Alpine release, pinned here
//! by its version and SHA-256 ([`APK_STATIC`]), fetched when it is needed
//! and never kept in the repository: apk-tools is licensed GPL-2.0-only, and
//! running it to compare answers does not distribute it. The keys are the
//! `alpine-keys` package of the same release, pinned the same way
//! ([`KEYS`]). Indexes are fetched as their mirror serves them, signed, and
//! apk.static verifies each with the keys as it loads it, so the harness
//! carries no verifier of its own.
//!
//! A directory [`fetch`] fills holds:
//!
//! - `apk.static`, the pinned apk, whose SHA-256 the package fixes and
//!   [`APK_STATIC_BINARY`] pins, and which runs only once it is checked
//!   against that pin ([`Layout::apk`]);
//! - `keys/`, the release's keys for the architecture, and any added;
//! - `architecture`, and `repositories`, each repository's URL on a line of
//!   its own, in the order configured;
//! - `repository-N/ARCH/APKINDEX.tar.gz`, each repository's index as its
//!   mirror served it, numbered from 1;
//! - `packages/`, the pinned packages as fetched, or as kept there before:
//!   a package there whose SHA-256 is its pin's is used rather than fetched
//!   again, so a directory or a cache that keeps it outlives the mirror's
//!   copy, and one with another digest is refused;
//! - `release/apk.static`, where asked for and where it differs from the
//!   pinned one, the apk.static the repositories offer, and nothing in
//!   `release/` otherwise: a fetch removes what an earlier one left there.
//!
//! A directory is reused as it is, by a later sweep or `case`. The pinned
//! apk is checked again before each use. The release's apk.static cannot
//! be: it is whatever the release offers, and the pinned apk checks it
//! against the release's signed index as it installs it, at fetch time; a
//! record of its digest made then would live in the directory, which is
//! what a reuse may have changed, and a check against it would be worth no
//! more than the directory. Instead, `fetch` prints its SHA-256, and a
//! sweep's results name the SHA-256 of the apk that answered, so a binary
//! changed between two runs shows in their stamps, and the shards of a run
//! answered by different binaries do not merge.
//!
//! Fetching runs `curl`, digests run `sha256sum`, packages are taken apart
//! by `tar`, and index archives decompressed by `gzip`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use ferrata::Dialect;
use ferrata::index;

use crate::apk::{Apk, Setting, TempDir, empty_root};
use crate::archive;
use crate::format::printed;

/// The mirror packages and indexes are fetched from.
pub const MIRROR: &str = "https://dl-cdn.alpinelinux.org/alpine";

/// One binary package of an Alpine release, pinned by its digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    /// The release, as its mirror path names it: `v3.24`.
    pub release: &'static str,
    /// The repository of the release that holds it.
    pub repository: &'static str,
    /// The architecture it is built for.
    pub architecture: &'static str,
    /// The package's name.
    pub name: &'static str,
    /// The package's version.
    pub version: &'static str,
    /// The SHA-256 of the package file, in hexadecimal.
    pub sha256: &'static str,
}

impl Pin {
    /// The package file's name.
    pub fn file_name(&self) -> String {
        format!("{}-{}.apk", self.name, self.version)
    }

    /// Where the mirror serves it.
    pub fn url(&self) -> String {
        format!(
            "{MIRROR}/{}/{}/{}/{}",
            self.release,
            self.repository,
            self.architecture,
            self.file_name()
        )
    }
}

/// apk.static: the release of apk-tools [`Dialect::V3_0`] was measured
/// against, as Alpine v3.24 ships it.
///
/// A stable release drops a package from its mirror when it moves to a newer
/// build, so this pin stops fetching once v3.24 moves past it; a copy kept
/// in a fetched directory's `packages/` still serves (see [`fetch_pinned`]).
/// The pin moves when the newer apk has been measured, with the dialect.
pub const APK_STATIC: Pin = Pin {
    release: "v3.24",
    repository: "main",
    architecture: "x86_64",
    name: "apk-tools-static",
    version: "3.0.9-r0",
    sha256: "0a1a0682375f66a83b7b903f6e4638ccfe312547d50d336b5f2be9fc391281be",
};

/// The SHA-256 of `sbin/apk.static` in the [`APK_STATIC`] package, which
/// the package fixes: what a fetched directory's `apk.static` is checked
/// against before it runs ([`Layout::apk`]).
pub const APK_STATIC_BINARY: &str =
    "9de54b4e3ea869a999842e716c9342e1b359566b161879d5002b715ed1c3a2f3";

/// The keys Alpine signs its indexes and packages with, as Alpine v3.24
/// ships them, every architecture's.
pub const KEYS: Pin = Pin {
    release: "v3.24",
    repository: "main",
    architecture: "x86_64",
    name: "alpine-keys",
    version: "2.6-r0",
    sha256: "dd211936d544f4050924ce8aec078d24e7b1b036ae70b30bd07867349587c708",
};

/// The repositories a sweep resolves over unless told otherwise: the pinned
/// release's `main` and `community`.
pub fn default_repositories() -> Vec<String> {
    ["main", "community"]
        .iter()
        .map(|repository| format!("{MIRROR}/{}/{repository}", APK_STATIC.release))
        .collect()
}

/// Runs `command`, failing with what it printed where it fails.
fn run(command: &mut Command) -> io::Result<Vec<u8>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{command:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

/// Fetches `url` into the file `path`, over HTTPS only.
///
/// # Errors
///
/// Fails where the transfer fails: of kind [`io::ErrorKind::NotFound`]
/// where the server answers that it holds nothing at `url` (HTTP 404 or
/// 410), which a transfer tried again does not change.
pub fn download(url: &str, path: &Path) -> io::Result<()> {
    let output = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=https", "--retry", "3"])
        .args(["--write-out", "%{http_code}", "--output"])
        .arg(path)
        .arg(url)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let kind = match String::from_utf8_lossy(&output.stdout).trim() {
        "404" | "410" => io::ErrorKind::NotFound,
        _ => io::ErrorKind::Other,
    };
    Err(io::Error::new(
        kind,
        format!(
            "fetching {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    ))
}

/// The SHA-256 of the file `path`, in hexadecimal.
pub fn sha256(path: &Path) -> io::Result<String> {
    let output = run(Command::new("sha256sum").arg("--binary").arg(path))?;
    let text = String::from_utf8_lossy(&output);
    text.split(' ')
        .next()
        .filter(|digest| digest.len() == 64)
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other(format!("sha256sum printed {text:?}")))
}

/// The package `pin` names, in `directory`: the file of its name there,
/// where its SHA-256 is the pin's, or else the package fetched from the
/// mirror, once its SHA-256 is checked. A file of the package's name in
/// `directory` is only ever one checked against the pin: a fetch is written
/// beside it and takes the name once checked.
///
/// # Errors
///
/// Fails where a file of the package's name in `directory` has another
/// digest, which is neither used nor replaced; where the mirror does not
/// serve the package, saying so plainly where it holds nothing at the
/// pin's URL, as it does once the release has moved past the package; and
/// where the mirror serves other bytes than the pin names.
pub fn fetch_pinned(pin: &Pin, directory: &Path) -> io::Result<PathBuf> {
    let path = directory.join(pin.file_name());
    if path.exists() {
        let digest = sha256(&path)?;
        if digest != pin.sha256 {
            return Err(io::Error::other(format!(
                "{} has SHA-256 {digest}, not the pinned {}: it is neither used nor replaced",
                path.display(),
                pin.sha256
            )));
        }
        return Ok(path);
    }
    let fetched = directory.join(format!("{}.part", pin.file_name()));
    if let Err(error) = download(&pin.url(), &fetched) {
        let _ = fs::remove_file(&fetched);
        if error.kind() == io::ErrorKind::NotFound {
            return Err(io::Error::other(format!(
                "the mirror no longer serves the pinned {}, and {} holds no copy of it: \
                 {} has moved past the pinned package, and the pin must move ({error})",
                pin.file_name(),
                directory.display(),
                pin.release
            )));
        }
        return Err(error);
    }
    let digest = sha256(&fetched)?;
    if digest != pin.sha256 {
        let _ = fs::remove_file(&fetched);
        return Err(io::Error::other(format!(
            "{} has SHA-256 {digest}, not the pinned {}",
            pin.url(),
            pin.sha256
        )));
    }
    fs::rename(&fetched, &path)?;
    Ok(path)
}

/// Unpacks `members` of the package `package` into `directory`.
fn unpack(package: &Path, directory: &Path, members: &[&str]) -> io::Result<()> {
    run(Command::new("tar")
        .arg("-xzf")
        .arg(package)
        .arg("-C")
        .arg(directory)
        .args(members))
    .map(|_| ())
}

/// The text of the index in the archive `archive`, as apk reads it: the
/// first `APKINDEX` member of the archive's tar stream, before the stream's
/// end ([`archive::member`]). In a signed index apk loads without a
/// warning, that is the signed member, so what is read is what apk
/// verified, and never a member appended after it.
pub fn index_text(archive: &Path) -> io::Result<String> {
    let stream = run(Command::new("gzip").arg("-dc").arg(archive))?;
    let text = archive::member(&stream, "APKINDEX")
        .ok_or_else(|| io::Error::other(format!("{}: no APKINDEX", archive.display())))?;
    String::from_utf8(text.to_vec())
        .map_err(|_| io::Error::other(format!("{}: not UTF-8", archive.display())))
}

/// What [`fetch`] fetched.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// The directory it fetched into.
    pub directory: PathBuf,
    /// Each repository's description, as apk printed it on loading it.
    pub descriptions: Vec<String>,
    /// The version of `apk-tools-static` the repositories offer, if any.
    pub offered: Option<String>,
    /// Where the offered apk.static was put, and the package it came from,
    /// where it was asked for and differs from the pinned one.
    pub release_apk: Option<(PathBuf, PathBuf)>,
}

/// A fetched directory's parts, as a sweep reads them.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The directory.
    pub directory: PathBuf,
    /// The architecture.
    pub architecture: String,
    /// Each repository's URL, in order.
    pub urls: Vec<String>,
    /// Each repository's local directory, in order.
    pub repositories: Vec<PathBuf>,
}

impl Layout {
    /// Reads the layout [`fetch`] left in `directory`.
    pub fn read(directory: &Path) -> io::Result<Layout> {
        // apk reads a relative path to its keys from its root, so every
        // path it is given is absolute.
        let directory = &fs::canonicalize(directory)?;
        let read = |name: &str| {
            fs::read_to_string(directory.join(name)).map_err(|error| {
                io::Error::other(format!("{}/{name}: {error}", directory.display()))
            })
        };
        let architecture = read("architecture")?.trim().to_owned();
        let urls: Vec<String> = read("repositories")?
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        let repositories = (1..=urls.len())
            .map(|index| directory.join(format!("repository-{index}")))
            .collect();
        Ok(Layout {
            directory: directory.to_owned(),
            architecture,
            urls,
            repositories,
        })
    }

    /// The pinned apk.static fetched into it, once its SHA-256 is the
    /// pin's, [`APK_STATIC_BINARY`]: nothing runs from a reused directory
    /// that the pin does not name.
    ///
    /// # Errors
    ///
    /// Fails where the file is missing, has another digest, or is not apk.
    pub fn apk(&self) -> io::Result<Apk> {
        let program = self.directory.join("apk.static");
        let digest = sha256(&program)?;
        if digest != APK_STATIC_BINARY {
            return Err(io::Error::other(format!(
                "{} has SHA-256 {digest}, not the pinned {APK_STATIC_BINARY}; fetch again",
                program.display()
            )));
        }
        Apk::new(program)
    }

    /// The keys indexes are verified with.
    pub fn keys(&self) -> PathBuf {
        self.directory.join("keys")
    }

    /// Each repository's index archive, in order.
    pub fn indexes(&self) -> Vec<PathBuf> {
        self.repositories
            .iter()
            .map(|repository| repository.join(&self.architecture).join("APKINDEX.tar.gz"))
            .collect()
    }

    /// The setting apk resolves in over these repositories, from `root`.
    pub fn setting<'a>(&'a self, root: &'a Path, keys: &'a Path) -> Setting<'a> {
        Setting {
            root,
            repositories: &self.repositories,
            architecture: &self.architecture,
            keys: Some(keys),
        }
    }
}

/// Fetches into `directory` the pinned apk.static and keys, and the index
/// of each repository of `urls` for `architecture`, then has apk.static
/// load the indexes, verifying each with the keys and any `extra_keys`.
/// With `release_apk`, also fetches the apk.static the repositories offer,
/// where it is another version than the pinned one, and has the pinned apk
/// install it, which verifies it against the index that offers it.
///
/// # Errors
///
/// Fails where a fetch fails, a pinned package's digest differs from its
/// pin, or apk does not load every index cleanly.
pub fn fetch(
    directory: &Path,
    architecture: &str,
    urls: &[String],
    extra_keys: &[PathBuf],
    release_apk: bool,
) -> io::Result<Fetched> {
    fs::create_dir_all(directory)?;
    let directory = &fs::canonicalize(directory)?;
    // An earlier fetch's release apk is not this one's, whatever this one
    // fetches.
    let earlier = directory.join("release");
    if earlier.exists() {
        fs::remove_dir_all(&earlier)?;
    }
    let packages = directory.join("packages");
    fs::create_dir_all(&packages)?;
    let unpacked = TempDir::new()?;

    let apk_package = fetch_pinned(&APK_STATIC, &packages)?;
    unpack(&apk_package, unpacked.path(), &["sbin/apk.static"])?;
    fs::copy(
        unpacked.path().join("sbin/apk.static"),
        directory.join("apk.static"),
    )?;

    let keys_package = fetch_pinned(&KEYS, &packages)?;
    unpack(&keys_package, unpacked.path(), &["usr/share/apk/keys"])?;
    let keys = directory.join("keys");
    if keys.exists() {
        fs::remove_dir_all(&keys)?;
    }
    fs::create_dir_all(&keys)?;
    let architecture_keys = unpacked
        .path()
        .join("usr/share/apk/keys")
        .join(architecture);
    for entry in fs::read_dir(&architecture_keys)
        .map_err(|error| io::Error::other(format!("no keys for {architecture}: {error}")))?
    {
        let path = entry?.path();
        // The architecture's directory links to the keys; copying follows
        // the links.
        if let Some(name) = path.file_name() {
            fs::copy(&path, keys.join(name))?;
        }
    }
    for key in extra_keys {
        let name = key
            .file_name()
            .ok_or_else(|| io::Error::other(format!("{} names no file", key.display())))?;
        fs::copy(key, keys.join(name))?;
    }

    for (at, url) in urls.iter().enumerate() {
        let local = directory
            .join(format!("repository-{}", at + 1))
            .join(architecture);
        fs::create_dir_all(&local)?;
        download(
            &format!(
                "{}/{architecture}/APKINDEX.tar.gz",
                url.trim_end_matches('/')
            ),
            &local.join("APKINDEX.tar.gz"),
        )?;
    }
    let mut list = urls.join("\n");
    list.push('\n');
    fs::write(directory.join("repositories"), list)?;
    fs::write(directory.join("architecture"), format!("{architecture}\n"))?;

    let layout = Layout::read(directory)?;
    let apk = layout.apk()?;
    let root = TempDir::new()?;
    empty_root(root.path())?;
    let keys = layout.keys();
    let descriptions = apk.update(&layout.setting(root.path(), &keys))?;

    // The apk.static the repositories offer: the latest version, from the
    // first repository offering it.
    let mut offered: Option<(ferrata::version::Version, usize)> = None;
    for (at, archive) in layout.indexes().iter().enumerate() {
        let text = index_text(archive)?;
        for record in index::read(Dialect::V3_0, &text).filter_map(Result::ok) {
            let record = record.record();
            if record.name() == APK_STATIC.name
                && offered
                    .as_ref()
                    .is_none_or(|(version, _)| record.version() > version)
            {
                offered = Some((record.version().clone(), at));
            }
        }
    }
    // The version as apk prints it and is asked for it.
    let offered = offered.map(|(version, at)| (printed(version.as_bytes()), at));
    let mut release = None;
    if let Some((version, at)) = &offered
        && release_apk
        && version != APK_STATIC.version
    {
        release = Some(fetch_offered(&layout, &apk, version, *at)?);
    }
    Ok(Fetched {
        directory: directory.to_owned(),
        descriptions,
        offered: offered.map(|(version, _)| version),
        release_apk: release,
    })
}

/// Fetches `apk-tools-static` at `version` from the repository numbered
/// `at` (from 0) of `layout`, and installs it with `apk`, which checks its
/// signature and its checksum against the index; returns where its
/// apk.static is, and the package.
fn fetch_offered(
    layout: &Layout,
    apk: &Apk,
    version: &str,
    at: usize,
) -> io::Result<(PathBuf, PathBuf)> {
    let file = format!("{}-{version}.apk", APK_STATIC.name);
    let (url, repository) = layout
        .urls
        .get(at)
        .zip(layout.repositories.get(at))
        .ok_or_else(|| io::Error::other("no such repository"))?;
    let package = repository.join(&layout.architecture).join(&file);
    download(
        &format!(
            "{}/{}/{file}",
            url.trim_end_matches('/'),
            layout.architecture
        ),
        &package,
    )?;
    let release = layout.directory.join("release");
    let root = release.join("root");
    fs::create_dir_all(&root)?;
    let keys = layout.keys();
    let output = apk
        .command(&layout.setting(&root, &keys))
        .args(["--usermode", "--initdb", "--no-scripts", "add"])
        .arg(format!("{}={version}", APK_STATIC.name))
        .output()?;
    let installed = root.join("sbin/apk.static");
    if !output.status.success() || !installed.exists() {
        return Err(io::Error::other(format!(
            "apk did not install {file}:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let binary = release.join("apk.static");
    fs::copy(&installed, &binary)?;
    Ok((binary, package))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oracle_is_the_release_the_dialect_was_measured_against() {
        let measured = Dialect::V3_0.measured_against();
        assert!(
            APK_STATIC.version.starts_with(&format!("{measured}-")),
            "{} pinned, the dialect measured against {measured}",
            APK_STATIC.version
        );
    }

    #[test]
    fn a_kept_package_serves_where_its_digest_is_the_pin_s() {
        let directory = TempDir::new().unwrap();
        // The SHA-256 of `kept\n`.
        let pin = Pin {
            release: "v0",
            repository: "main",
            architecture: "x86_64",
            name: "kept",
            version: "1-r0",
            sha256: "78051faade059d70866df6a3fb83ef348721fd74a87e93ef95c493f87d0d236b",
        };
        let path = directory.path().join(pin.file_name());
        fs::write(&path, "kept\n").unwrap();
        assert_eq!(sha256(&path).unwrap(), pin.sha256);
        assert_eq!(fetch_pinned(&pin, directory.path()).unwrap(), path);
        // Another file of its name is refused, and left as it is.
        fs::write(&path, "poisoned\n").unwrap();
        let error = fetch_pinned(&pin, directory.path()).unwrap_err();
        assert!(
            error.to_string().contains("neither used nor replaced"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "poisoned\n");
    }

    #[test]
    fn a_directory_s_apk_runs_only_where_it_is_the_pinned_one() {
        let directory = TempDir::new().unwrap();
        let layout = Layout {
            directory: directory.path().to_owned(),
            architecture: "x86_64".into(),
            urls: Vec::new(),
            repositories: Vec::new(),
        };
        assert!(layout.apk().is_err(), "no apk.static at all");
        fs::write(directory.path().join("apk.static"), "#!/bin/sh\n").unwrap();
        let error = layout.apk().unwrap_err();
        assert!(error.to_string().contains("not the pinned"), "{error}");
        // The pinned binary itself, where the tests are given it: linked,
        // since a copy this process wrote could be busy when run.
        #[cfg(unix)]
        if let Some(pinned) = std::env::var_os("FERRATA_APK_STATIC")
            && sha256(Path::new(&pinned)).unwrap() == APK_STATIC_BINARY
        {
            let program = directory.path().join("apk.static");
            fs::remove_file(&program).unwrap();
            std::os::unix::fs::symlink(&pinned, &program).unwrap();
            assert!(
                layout
                    .apk()
                    .unwrap()
                    .banner()
                    .starts_with("apk-tools 3.0.9")
            );
        }
    }

    #[test]
    fn only_the_first_index_of_an_archive_is_read() {
        let directory = TempDir::new().unwrap();
        let index = archive::tar(&[("DESCRIPTION", b"d"), ("APKINDEX", b"P:first\n")]);
        let second = archive::tar(&[("APKINDEX", b"P:second\n")]);
        // A second gzip member appended after the index's end blocks.
        let appended = directory.path().join("appended.tar.gz");
        fs::write(
            &appended,
            [archive::gzip(&index), archive::gzip(&second)].concat(),
        )
        .unwrap();
        assert_eq!(index_text(&appended).unwrap(), "P:first\n");
        // A second member of the name in the same stream, the end blocks
        // gone.
        let unended = directory.path().join("unended.tar.gz");
        let stream = [&index[..index.len() - 1024], &second[..]].concat();
        fs::write(&unended, archive::gzip(&stream)).unwrap();
        assert_eq!(index_text(&unended).unwrap(), "P:first\n");
    }

    #[test]
    fn a_pin_names_its_file_on_the_mirror() {
        assert_eq!(
            APK_STATIC.url(),
            "https://dl-cdn.alpinelinux.org/alpine/v3.24/main/x86_64/apk-tools-static-3.0.9-r0.apk"
        );
    }
}
