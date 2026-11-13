//! Running a scenario with `apk.static`.
//!
//! A scenario is laid out in a directory, as [`materialize`] describes, and
//! apk is run on it:
//!
//! ```text
//! apk --root DIR/root --allow-untrusted --no-cache --simulate --arch x86_64 \
//!     --repositories-file /dev/null --repository DIR/repository-1 ... \
//!     --no-interactive add -- REQUEST...
//! ```
//!
//! The root is written by hand: an empty one is an empty world and an empty
//! installed database, which apk reads as it reads one it made itself, and
//! needs no privilege. `--simulate` fetches nothing, so the repositories hold
//! only their indexes.
//!
//! [`Apk::add`] runs the same command in any [`Setting`]: a sweep's
//! repositories are the signed indexes of a release, which apk verifies with
//! the release's keys as it loads them, in place of `--allow-untrusted`.

use std::fs::{self, File};
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::archive;
use crate::format::{Action, Answer, Outcome, Record, Repository, Scenario, Step};

/// The architecture every scenario is resolved for.
pub const ARCHITECTURE: &str = "x86_64";

/// An `apk.static` binary.
#[derive(Debug, Clone)]
pub struct Apk {
    program: PathBuf,
    banner: String,
}

impl Apk {
    /// The binary at `program`, after asking it which apk it is.
    pub fn new(program: impl Into<PathBuf>) -> io::Result<Apk> {
        let program = program.into();
        let output = Command::new(&program).arg("--version").output()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let banner = text.split(',').next().unwrap_or_default().trim().to_owned();
        if !banner.starts_with("apk-tools ") {
            return Err(io::Error::other(format!(
                "{} is not apk: {text:?}",
                program.display()
            )));
        }
        Ok(Apk { program, banner })
    }

    /// The binary `FERRATA_APK_STATIC` names, if it names one.
    pub fn from_env() -> Option<io::Result<Apk>> {
        std::env::var_os("FERRATA_APK_STATIC").map(Apk::new)
    }

    /// Which apk this is, as `apk-tools 3.0.9-r0`.
    pub fn banner(&self) -> &str {
        &self.banner
    }

    /// The binary.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// apk's answer to `scenario`.
    pub fn answer(&self, scenario: &Scenario) -> io::Result<Answer> {
        let directory = TempDir::new()?;
        materialize(scenario, directory.path())?;
        self.answer_in(scenario, directory.path())
    }

    /// apk's answer to `scenario`, already laid out in `directory`.
    pub fn answer_in(&self, scenario: &Scenario, directory: &Path) -> io::Result<Answer> {
        let repositories: Vec<PathBuf> = (1..=scenario.repositories.len())
            .map(|index| directory.join(format!("repository-{index}")))
            .collect();
        let setting = Setting {
            root: &directory.join("root"),
            repositories: &repositories,
            architecture: ARCHITECTURE,
            keys: None,
        };
        self.add(&setting, &scenario.request, &directory.join("apk-output"))
    }

    /// The command apk is run as in `setting`, up to the applet.
    pub(crate) fn command(&self, setting: &Setting<'_>) -> Command {
        let mut command = Command::new(&self.program);
        command.arg("--root").arg(setting.root);
        match setting.keys {
            Some(keys) => command.arg("--keys-dir").arg(keys),
            None => command.arg("--allow-untrusted"),
        };
        command
            .args(["--no-cache", "--arch", setting.architecture])
            .args(["--repositories-file", "/dev/null"]);
        for repository in setting.repositories {
            command.arg("--repository").arg(repository);
        }
        command.arg("--no-interactive");
        command
    }

    /// apk's answer to `apk add --simulate` of `request` in `setting`.
    ///
    /// What apk prints goes to the file `output`, its standard output and
    /// standard error together, in the order printed. A path under the
    /// directory holding the root is written relative to it, so that an
    /// answer does not depend on where apk was run.
    ///
    /// # Errors
    ///
    /// Fails where apk cannot be run, or gives no answer: it is ended by a
    /// signal, fails without the `ERROR:` line a refusal prints, or succeeds
    /// with its output cut short. Each is a failure of the tool, as a crash
    /// or a full disk is, not apk's refusal of the request.
    pub fn add(
        &self,
        setting: &Setting<'_>,
        request: &[String],
        output: &Path,
    ) -> io::Result<Answer> {
        let file = File::create(output)?;
        let status = self
            .command(setting)
            .args(["--simulate", "add", "--"])
            .args(request)
            .stdout(file.try_clone()?)
            .stderr(file)
            .status()?;
        let mut text = fs::read_to_string(output)?;
        if let Some(base) = setting.root.parent() {
            text = text.replace(&format!("{}/", base.display()), "");
        }
        let code = status.code().ok_or_else(|| {
            io::Error::other(format!(
                "apk did not exit, {status}, having printed {text:?}"
            ))
        })?;
        read_output(&self.banner, &text, code).map_err(io::Error::other)
    }

    /// Has apk load the indexes of `setting`'s repositories, as `apk update`
    /// does, and returns each one's description, in the order of the
    /// repositories.
    ///
    /// # Errors
    ///
    /// Fails where apk does not load each index cleanly: one it cannot
    /// verify with `setting`'s keys is one apk passes over with a warning,
    /// and a sweep over it would compare nothing.
    pub fn update(&self, setting: &Setting<'_>) -> io::Result<Vec<String>> {
        let output = self.command(setting).arg("update").output()?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        let fail = || io::Error::other(format!("apk did not load every index cleanly:\n{text}"));
        let warned = text
            .lines()
            .any(|line| line.starts_with("WARNING") || line.starts_with("ERROR"));
        if !output.status.success() || warned {
            return Err(fail());
        }
        // apk prints each index's description and, in brackets, the
        // repository it loaded it from; apk 2.14 in an order of its own.
        setting
            .repositories
            .iter()
            .map(|repository| {
                let suffix = format!(" [{}]", repository.display());
                text.lines()
                    .find_map(|line| line.strip_suffix(&suffix))
                    .map(str::to_owned)
                    .ok_or_else(fail)
            })
            .collect()
    }
}

/// Where apk resolves: its root, its repositories in the order configured,
/// the architecture, and the directory of keys it verifies indexes with, or
/// none, to read them unsigned.
#[derive(Debug, Clone, Copy)]
pub struct Setting<'a> {
    /// The root, which holds an installed database and a world.
    pub root: &'a Path,
    /// Each repository: a directory or URL holding `ARCH/APKINDEX.tar.gz`.
    pub repositories: &'a [PathBuf],
    /// The architecture resolved for.
    pub architecture: &'a str,
    /// The keys indexes are verified with; `None` reads them unsigned.
    pub keys: Option<&'a Path>,
}

/// Writes a root in `root` by hand: the installed database `installed`, the
/// text of its records, and the world `world`, one entry to a line, which
/// apk reads as a root it made itself; and the cache directory, which apk
/// 2.14 opens to load indexes even with `--no-cache`.
pub fn write_root(root: &Path, installed: &str, world: &[impl AsRef<str>]) -> io::Result<()> {
    let database = root.join("lib/apk/db");
    let configuration = root.join("etc/apk");
    fs::create_dir_all(&database)?;
    fs::create_dir_all(&configuration)?;
    fs::create_dir_all(root.join("var/cache/apk"))?;
    fs::write(database.join("installed"), installed)?;
    let mut text = String::new();
    for entry in world {
        text.push_str(entry.as_ref());
        text.push('\n');
    }
    fs::write(configuration.join("world"), text)
}

/// Writes an empty root in `root`: an empty installed database and an
/// empty world.
pub fn empty_root(root: &Path) -> io::Result<()> {
    write_root(root, "", &[] as &[&str])
}

/// Reads what apk printed, exiting with `code`, into an answer.
///
/// apk ends what it prints on success with a line `OK: ` and the size of
/// the root. Output without one was cut short, as it is when the disk fills
/// while apk writes it, and is an error, not an answer that changes less.
/// apk refuses a request with a line beginning `ERROR: ` (`unable to select
/// packages:` and what it could not select, or an entry that does not read)
/// and a code other than 0; a failure without one is not apk's answer.
fn read_output(banner: &str, text: &str, code: i32) -> Result<Answer, String> {
    let success = code == 0;
    let mut messages = Vec::new();
    let mut steps = Vec::new();
    let mut finished = false;
    for line in text.lines() {
        if line.starts_with("OK: ") {
            finished = true;
            continue;
        }
        if line.is_empty() {
            continue;
        }
        match apk_step(line)? {
            Some(step) if success => steps.push(step),
            _ => messages.push(line.to_owned()),
        }
    }
    if success && !finished {
        return Err(format!(
            "apk succeeded, and its output does not end as apk's does: {text:?}"
        ));
    }
    if !success && !messages.iter().any(|line| line.starts_with("ERROR: ")) {
        return Err(format!(
            "apk failed with code {code}, and its output does not say why, as a refusal does: {text:?}"
        ));
    }
    let outcome = if success {
        Outcome::Changes(steps)
    } else {
        Outcome::Refused
    };
    Ok(Answer {
        apk: banner.to_owned(),
        messages,
        outcome,
    })
}

/// Reads one of apk's progress lines, `(1/3) Installing name (1.0-r0)`, as a
/// step, refusing a verb it does not know rather than passing it by, and a
/// line that names a version replaced where the verb replaces nothing, or
/// none where it does ([`Action::replaces`]).
fn apk_step(line: &str) -> Result<Option<Step>, String> {
    let Some((counter, rest)) = line
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(") "))
    else {
        return Ok(None);
    };
    // apk pads the counter to the width of the total: `(  1/349)`.
    let counter = counter.trim_start();
    if !counter.contains('/') || !counter.chars().all(|c| c.is_ascii_digit() || c == '/') {
        return Ok(None);
    }
    // apk notes what it could not do in brackets before the verb.
    let mut rest = rest;
    while let Some(after) = rest
        .strip_prefix('[')
        .and_then(|note| note.split_once("] "))
    {
        rest = after.1;
    }
    let fail = || format!("an apk step ferrata does not know: {line:?}");
    let (verb, rest) = rest.split_once(' ').ok_or_else(fail)?;
    let (name, details) = rest.split_once(" (").ok_or_else(fail)?;
    let details = details.strip_suffix(')').ok_or_else(fail)?;
    let action = match verb {
        "Installing" => Action::Install,
        "Upgrading" => Action::Upgrade,
        "Downgrading" => Action::Downgrade,
        "Replacing" => Action::Replace,
        "Reinstalling" => Action::Reinstall,
        "Purging" | "Deleting" | "Removing" => Action::Remove,
        _ => return Err(fail()),
    };
    let (from, version) = match details.split_once(" -> ") {
        Some((from, to)) => (Some(from.to_owned()), to.to_owned()),
        None => (None, details.to_owned()),
    };
    if from.is_some() != action.replaces() {
        return Err(fail());
    }
    Ok(Some(Step {
        action,
        name: name.to_owned(),
        from,
        version,
    }))
}

/// The scenario with every record completed with its identity, and without
/// its answer: what a resolver is given.
pub fn completed(scenario: &Scenario) -> Scenario {
    let mut completed = scenario.clone();
    for repository in &mut completed.repositories {
        if let Repository::Records(records) = repository {
            *records = records.iter().map(Record::completed).collect();
        }
    }
    completed.installed = scenario.installed.iter().map(Record::completed).collect();
    completed.answer = None;
    completed
}

/// The text of an index or an installed database holding `records`, each
/// record's lines and a blank line after it: what
/// [`format::index_records`](crate::format::index_records) reads back. The
/// text of an index fetched from a mirror is
/// [`fetch::index_text`](crate::fetch::index_text).
pub fn records_text(records: &[Record]) -> String {
    let mut text = String::new();
    for record in records {
        text.push_str(&record.text());
        text.push('\n');
    }
    text
}

/// Lays `scenario` out in `directory`, for apk and for any other resolver:
///
/// - `scenario`: the scenario, its records completed, without its answer;
/// - `repository-N/APKINDEX`: each repository's index, numbered from 1 in
///   order, and `repository-N/x86_64/APKINDEX.tar.gz`, the same index as apk
///   reads it;
/// - `request`: the request, one argument to a line;
/// - `root/lib/apk/db/installed` and `root/etc/apk/world`: the installed set.
pub fn materialize(scenario: &Scenario, directory: &Path) -> io::Result<()> {
    let completed = completed(scenario);
    fs::write(directory.join("scenario"), completed.to_string())?;
    for (index, repository) in completed.repositories.iter().enumerate() {
        let text = match repository {
            Repository::Records(records) => records_text(records),
            Repository::Verbatim(text) => text.clone(),
        };
        let path = directory.join(format!("repository-{}", index + 1));
        fs::create_dir_all(path.join(ARCHITECTURE))?;
        fs::write(path.join("APKINDEX"), &text)?;
        let archive = archive::tar(&[("DESCRIPTION", b"scenario"), ("APKINDEX", text.as_bytes())]);
        fs::write(
            path.join(ARCHITECTURE).join("APKINDEX.tar.gz"),
            archive::gzip(&archive),
        )?;
    }
    let mut request = completed.request.join("\n");
    request.push('\n');
    fs::write(directory.join("request"), request)?;
    write_root(
        &directory.join("root"),
        &records_text(&completed.installed),
        &completed.world,
    )
}

/// A directory removed when dropped.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// A new, empty directory under the system's temporary directory, made
    /// for this alone: its name ends in a random number, it is created only
    /// where nothing is, never over or in place of what is there, and on
    /// Unix only its owner may enter it.
    ///
    /// # Errors
    ///
    /// Fails where the directory cannot be created, or every name tried is
    /// taken.
    pub fn new() -> io::Result<TempDir> {
        const TRIES: usize = 16;
        let base = std::env::temp_dir();
        for _ in 0..TRIES {
            let path = base.join(format!("ferrata-scenario-{:016x}", unpredictable()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            match builder.create(&path) {
                Ok(()) => return Ok(TempDir { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{}: {TRIES} names for a temporary directory taken",
                base.display()
            ),
        ))
    }

    /// Where it is.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A number another process cannot predict: a hash under keys the
/// standard library draws from the system's source of randomness, each
/// [`RandomState`] its own, of a count, the process and the time.
fn unpredictable() -> u64 {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_usize(COUNT.fetch_add(1, Ordering::Relaxed));
    hasher.write_u32(std::process::id());
    if let Ok(since) = SystemTime::now().duration_since(UNIX_EPOCH) {
        hasher.write_u128(since.as_nanos());
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_temporary_directory_is_new_and_its_own() {
        let (one, two) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        assert_ne!(one.path(), two.path());
        assert_eq!(fs::read_dir(one.path()).unwrap().count(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(one.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let path = one.path().to_owned();
        drop(one);
        assert!(!path.exists());
    }

    #[test]
    fn progress_lines_read_as_steps() {
        let text = "WARNING: something\n(1/3) Purging a (1.0-r0)\n\
                    (2/3) Upgrading lib (1.0-r0 -> 2.0-r0)\n\
                    (3/3) [APK unavailable, skipped] Reinstalling re (1.0-r0)\nOK: 0 B in 3 packages\n";
        let answer = read_output("apk-tools 3.0.9-r0", text, 0).unwrap();
        assert_eq!(answer.messages, ["WARNING: something"]);
        assert_eq!(
            answer.lines(),
            "| WARNING: something\nremove a 1.0-r0\nupgrade lib 1.0-r0 2.0-r0\nreinstall re 1.0-r0\n"
        );
    }

    #[test]
    fn a_padded_counter_is_read() {
        let answer = read_output(
            "apk",
            "(  1/349) Installing tzdata (2026f-r0)\nOK: 1 MiB in 1 packages\n",
            0,
        )
        .unwrap();
        assert_eq!(answer.lines(), "install tzdata 2026f-r0\n");
    }

    #[test]
    fn output_cut_short_is_no_answer() {
        assert!(read_output("apk", "", 0).is_err());
        assert!(read_output("apk", "(1/2) Installing a (1.0-r0)\n", 0).is_err());
        assert!(read_output("apk", "OK: 0 B in 0 packages\n", 0).is_ok());
    }

    #[test]
    fn an_unknown_verb_is_an_error() {
        assert!(read_output("apk", "(1/1) Frobbing a (1.0-r0)\n", 0).is_err());
    }

    #[test]
    fn a_step_names_a_version_replaced_where_its_verb_replaces_one() {
        let ok = |line: &str| read_output("apk", &format!("{line}\nOK: 0 B\n"), 0);
        assert!(ok("(1/1) Replacing a (1.0-r0 -> 1.0-r0)").is_ok());
        assert!(ok("(1/1) Reinstalling a (1.0-r0)").is_ok());
        assert!(ok("(1/1) Replacing a (1.0-r0)").is_err());
        assert!(ok("(1/1) Upgrading a (2.0-r0)").is_err());
        assert!(ok("(1/1) Installing a (1.0-r0 -> 2.0-r0)").is_err());
        assert!(ok("(1/1) Purging a (1.0-r0 -> 2.0-r0)").is_err());
        // What apk_step reads, Step::parse reads back.
        let answer = ok("(1/2) Downgrading a (2 -> 1)\n(2/2) Installing b (1)").unwrap();
        assert_eq!(Answer::parse_lines("apk", &answer.lines()).unwrap(), answer);
    }

    #[test]
    fn a_failure_apk_explains_is_a_refusal() {
        let text =
            "ERROR: unable to select packages:\n  b (no such package):\n    required by: a-1[b]\n";
        let answer = read_output("apk", text, 2).unwrap();
        assert_eq!(answer.outcome, Outcome::Refused);
        assert_eq!(
            answer.lines(),
            "| ERROR: unable to select packages:\n|   b (no such package):\n|     required by: a-1[b]\nrefused\n"
        );
    }

    #[test]
    fn a_failure_apk_does_not_explain_is_no_answer() {
        assert!(read_output("apk", "", 1).is_err());
        assert!(read_output("apk", "Segmentation fault\n", 139).is_err());
        assert!(read_output("apk", "WARNING: something\n", 1).is_err());
    }

    /// apk's answer from a stand-in that answers `--version` as apk does
    /// and runs `script` for the request.
    #[cfg(unix)]
    #[test]
    fn an_apk_ended_by_a_signal_or_failing_unexplained_gives_no_answer() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TempDir::new().unwrap();
        let scenario = Scenario {
            repositories: vec![Repository::Records(Vec::new())],
            request: vec!["a".into()],
            ..Scenario::default()
        };
        let cases = [
            ("killed", "kill -KILL $$", false),
            ("silent", "exit 1", false),
            (
                "refusing",
                "echo 'ERROR: unable to select packages:'; exit 1",
                true,
            ),
        ];
        // Each stand-in is written by `cp`, another process. A file this
        // one held open for writing would be held too by each process
        // another test thread starts meanwhile, until it runs its own
        // program, and running the file then fails as busy.
        let mut programs = Vec::new();
        for (name, script, _) in &cases {
            let text = directory.path().join(format!("{name}.sh"));
            fs::write(
                &text,
                format!(
                    "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'apk-tools 0-r0, compiled for x86_64.'; exit 0; fi\n{script}\n"
                ),
            )
            .unwrap();
            let program = directory.path().join(name);
            let copied = Command::new("cp").arg(&text).arg(&program).status();
            assert!(copied.unwrap().success());
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            programs.push(program);
        }
        for ((name, _, answers), program) in cases.iter().zip(programs) {
            let answer = Apk::new(program).unwrap().answer(&scenario);
            assert_eq!(answer.is_ok(), *answers, "{name}: {answer:?}");
        }
    }
}
