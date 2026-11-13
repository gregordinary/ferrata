//! The sweep: requests over real indexes, each resolved by apk.static and
//! by ferrata in one run, and the two answers compared, set and sequence.
//!
//! A sweep runs over a directory [`fetch`](crate::fetch) filled. It asks
//! apk to load the indexes first, which verifies each with the release's
//! keys, and reads the same files for ferrata. Then each request is resolved
//! by both, `apk add --simulate` into an empty root and ferrata's resolver,
//! by several workers at once; each result is sent whole to one writer,
//! which writes the results in the order of the requests, so no two results
//! interleave and two runs over the same input write the same file.
//!
//! The requests are each name a record carries, resolved alone; sets of
//! names drawn at random from a seed, resolved together; or layered pairs
//! drawn from a seed, each a request over a root an earlier request made.
//! A sweep may take a shard of them, so that a hosted job with a time limit
//! runs one shard. A name resolved alone belongs to the shard its [`hash`]
//! selects, so the shards of a run divide the names the same way whatever
//! other names the repositories hold. Drawn sets and layered pairs are dealt
//! out among the shards in the order the seed draws them
//! ([`Shard::takes`]), which the seed and the count fix.
//!
//! A layered pair is a base and a request. The base is one name, which apk
//! installs into an empty root; its packages' records, in the order apk
//! installs them, are written as the root's installed database by hand, and
//! the name as its world, as [`write_root`] writes a root. The request, a
//! second name, is then resolved over that root by apk and by ferrata, and
//! the changes each makes are compared: what each installs, upgrades,
//! replaces and removes, and in what order. The base may come from another
//! fetched directory, an older release's repositories, so that the request
//! over the newer ones upgrades and removes what the older release
//! installed; and the request may ask for the base's package at a later
//! version than the one installed, where the repositories offer one, as an
//! upgrade does.
//!
//! The results file begins with what produced it, every line after `# `:
//! ferrata's commit (with `+modified` where the tree differed from it), the
//! apk that answered and the SHA-256 of its binary, the architecture, each repository's URL, its
//! description as apk printed it, and the SHA-256 of its index archive, and
//! the same for the repositories a layered pair's base came from, where
//! they are others. One line per request follows, its fields separated by
//! tabs:
//!
//! ```text
//! REQUEST  VERDICT  APK  FERRATA
//! ```
//!
//! The request's entries are separated by spaces; a layered pair's are its
//! base's, `>` and the request's, as [`Asked`] writes them. The verdict is
//! `same` (the same changes in the same order, or both refused), `order`
//! (the same changes in another order), `set` (other changes), or `refusal`
//! (one side refused). Each answer is `refused`, or the changes in order,
//! separated by spaces: a package installed as `name=version`, and any other
//! change with its kind before it, as a scenario's answer writes it: the
//! version installed before and the one after for an upgrade, a downgrade
//! and a replacement (`Upgrade:name=1->2`), and the version for any other
//! (`Remove:name=1`, the one removed); ferrata's is `=` where it is apk's. A
//! last line counts the verdicts, so a file without it was cut short. A
//! shard's request line ends `, shard I of N`, and [`Results::merge`] reads
//! the shards of a run together, each once.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use ferrata::index::{self, Located};
use ferrata::record::Record;
use ferrata::{Dialect, Resolver};

use crate::apk::{self, Apk, Setting, TempDir, empty_root, write_root};
use crate::fetch::{Layout, index_text, sha256};
use crate::format::{self, Action, Answer, Outcome, Repository, Scenario, printed};
use crate::random::{Rng, Source, hash};
use crate::resolve;

/// The commit the harness was built from, with `+modified` where the tree
/// differed from it, or `unknown` outside a git checkout.
pub const COMMIT: &str = env!("FERRATA_COMMIT");

/// Which requests a sweep makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requests {
    /// Each name a record carries, alone.
    Each,
    /// `count` sets of `size` names, drawn at random from `seed`.
    Sets {
        /// How many sets.
        count: usize,
        /// How many names in each.
        size: usize,
        /// The seed they are drawn from.
        seed: u64,
    },
    /// `count` layered pairs, drawn from `seed`: a name apk installs into
    /// an empty root, and another name requested over that root.
    Layered {
        /// How many pairs.
        count: usize,
        /// The seed they are drawn from.
        seed: u64,
        /// Whether the request also asks for the base's package at a later
        /// version than the one installed, where the sweep's repositories
        /// offer one.
        upgrade: bool,
    },
}

/// How many names a layered pair draws for its base before it gives up:
/// a name apk refuses to install alone makes no root, and the pair draws
/// another.
const BASE_DRAWS: usize = 16;

/// What a sweep asks of `apk add`: `entries`, over the root `base` makes
/// (see the [module documentation](self)), or over an empty root where
/// `base` is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The base's entries, installed into an empty root and made its world.
    pub base: Vec<String>,
    /// The entries asked over the root.
    pub entries: Vec<String>,
}

impl Asked {
    /// Reads a request as a results file writes it. `>` alone cannot be an
    /// entry, which needs a name, so it separates a base from a request.
    pub fn parse(text: &str) -> Asked {
        let words: Vec<String> = text
            .split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect();
        match words.iter().position(|word| word == ">") {
            Some(at) => Asked {
                base: words[..at].to_vec(),
                entries: words[at + 1..].to_vec(),
            },
            None => Asked {
                base: Vec::new(),
                entries: words,
            },
        }
    }
}

impl std::fmt::Display for Asked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.base.is_empty() {
            write!(f, "{} > ", self.base.join(" "))?;
        }
        f.write_str(&self.entries.join(" "))
    }
}

/// One of `count` shards, numbered from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shard {
    /// Which shard, from 1.
    pub index: usize,
    /// How many shards.
    pub count: usize,
}

impl Shard {
    /// Reads `I/N`.
    pub fn parse(text: &str) -> Option<Shard> {
        let (index, count) = text.split_once('/')?;
        let shard = Shard {
            index: index.parse().ok()?,
            count: count.parse().ok()?,
        };
        (1..=shard.count).contains(&shard.index).then_some(shard)
    }

    /// Whether the name `name` falls in this shard.
    pub fn holds(&self, name: &str) -> bool {
        hash(name.bytes()) % self.count as u64 == (self.index - 1) as u64
    }

    /// Whether the request drawn `at`-th, from 0, falls in this shard: the
    /// drawn requests are dealt out among the shards in turn.
    pub fn takes(&self, at: usize) -> bool {
        at % self.count == self.index - 1
    }
}

impl std::fmt::Display for Shard {
    /// Writes the shard as a results file's request line names it: `shard 3
    /// of 8`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "shard {} of {}", self.index, self.count)
    }
}

/// How two answers to one request compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// The same packages in the same order, or both refused.
    Same,
    /// The same packages in another order.
    Order,
    /// Other packages.
    Set,
    /// One side refused and the other did not.
    Refusal,
}

impl Verdict {
    /// Every verdict, in the order a summary counts them.
    pub const ALL: [Verdict; 4] = [
        Verdict::Same,
        Verdict::Order,
        Verdict::Set,
        Verdict::Refusal,
    ];

    /// How a results file writes it.
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Same => "same",
            Verdict::Order => "order",
            Verdict::Set => "set",
            Verdict::Refusal => "refusal",
        }
    }

    fn from_word(word: &str) -> Option<Verdict> {
        Verdict::ALL
            .into_iter()
            .find(|verdict| verdict.word() == word)
    }

    /// How `ferrata`'s answer compares with `apk`'s.
    pub fn of(apk: &Answer, ferrata: &Answer) -> Verdict {
        match (&apk.outcome, &ferrata.outcome) {
            (Outcome::Refused, Outcome::Refused) => Verdict::Same,
            (Outcome::Changes(ours), Outcome::Changes(theirs)) => {
                if ours == theirs {
                    Verdict::Same
                } else if apk.same_steps(ferrata) {
                    Verdict::Order
                } else {
                    Verdict::Set
                }
            }
            _ => Verdict::Refusal,
        }
    }
}

/// An answer as a results file writes it (see the
/// [module documentation](self)): every part of a step [`Verdict::of`]
/// compares.
pub fn written(answer: &Answer) -> String {
    match &answer.outcome {
        Outcome::Refused => "refused".to_owned(),
        Outcome::Changes(steps) => {
            let mut text = String::new();
            for step in steps {
                if !text.is_empty() {
                    text.push(' ');
                }
                if step.action != Action::Install {
                    let _ = write!(text, "{:?}:", step.action);
                }
                let _ = write!(text, "{}=", step.name);
                if let Some(from) = &step.from {
                    let _ = write!(text, "{from}->");
                }
                text.push_str(&step.version);
            }
            text
        }
    }
}

/// How many requests had each verdict.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts(BTreeMap<Verdict, usize>);

impl Counts {
    /// Counts one more `verdict`.
    pub fn add(&mut self, verdict: Verdict) {
        *self.0.entry(verdict).or_default() += 1;
    }

    /// How many had `verdict`.
    pub fn get(&self, verdict: Verdict) -> usize {
        self.0.get(&verdict).copied().unwrap_or_default()
    }

    /// How many in all.
    pub fn total(&self) -> usize {
        self.0.values().sum()
    }
}

impl std::fmt::Display for Counts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} requests:", self.total())?;
        for (at, verdict) in Verdict::ALL.into_iter().enumerate() {
            let separator = if at == 0 { "" } else { "," };
            write!(f, "{separator} {} {}", self.get(verdict), verdict.word())?;
        }
        Ok(())
    }
}

/// What a sweep is asked to do.
#[derive(Debug, Clone)]
pub struct Options {
    /// The apk.static to compare with; the directory's pinned one if
    /// `None`.
    pub apk: Option<PathBuf>,
    /// The requests.
    pub requests: Requests,
    /// The shard to run, or all of them.
    pub shard: Option<Shard>,
    /// How many requests to resolve at once.
    pub jobs: usize,
    /// A fetched directory whose repositories a layered pair's base is
    /// installed from, in place of the sweep's own.
    pub base: Option<PathBuf>,
}

/// The records of each repository of `layout`, with the lines that stamp
/// them: each repository's URL, its description, and its index's digest.
fn repositories(
    layout: &Layout,
    descriptions: &[String],
) -> io::Result<(Vec<Vec<Record>>, Vec<String>)> {
    let mut repositories = Vec::new();
    let mut stamps = Vec::new();
    for ((url, archive), description) in layout.urls.iter().zip(layout.indexes()).zip(descriptions)
    {
        let text = index_text(&archive)?;
        let records = index::read(Dialect::V3_0, &text)
            .map(|item| item.map(Located::into_record))
            .collect::<Result<Vec<Record>, _>>()
            .map_err(|error| io::Error::other(format!("{url}: {error}")))?;
        stamps.push(format!(
            "repository {url} {description} sha256:{}",
            sha256(&archive)?
        ));
        repositories.push(records);
    }
    Ok((repositories, stamps))
}

/// Each record's text in the indexes of `layout`, by name and version: of
/// two records naming the same, the first repository's, and the one listed
/// first in it, as apk chooses between them. A step of apk's answer names
/// the record it installs this way.
fn record_texts(layout: &Layout) -> io::Result<BTreeMap<(String, String), String>> {
    let mut texts = BTreeMap::new();
    for archive in layout.indexes() {
        let text = index_text(&archive)?;
        for located in index::read(Dialect::V3_0, &text) {
            let located = located
                .map_err(|error| io::Error::other(format!("{}: {error}", archive.display())))?;
            let span = text.get(located.span()).unwrap_or_default();
            let record = located.record();
            // As a step of apk's answer names it.
            texts
                .entry((
                    printed(record.name().as_bytes()),
                    printed(record.version().as_bytes()),
                ))
                .or_insert_with(|| {
                    let mut record_text = span.trim_end_matches('\n').to_owned();
                    record_text.push_str("\n\n");
                    record_text
                });
        }
    }
    Ok(texts)
}

/// The names the records of `repositories` carry, sorted: each that is
/// text, as a request and a results file write it, which every name of
/// Alpine's indexes is.
fn names_of(repositories: &[Vec<Record>]) -> Vec<&str> {
    let names: BTreeSet<&str> = repositories
        .iter()
        .flatten()
        .filter_map(|record| record.name().to_str())
        .collect();
    names.into_iter().collect()
}

/// One request of a sweep, before it is made.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    /// Entries resolved together into an empty root.
    Fresh(Vec<String>),
    /// A layered pair, drawn from its own seed.
    Pair(u64),
}

/// The requests `options` makes over `names`, which are sorted.
fn requests(names: &[&str], options: &Options) -> Vec<Job> {
    match options.requests {
        Requests::Each => names
            .iter()
            .filter(|name| options.shard.is_none_or(|shard| shard.holds(name)))
            .map(|name| Job::Fresh(vec![(*name).to_owned()]))
            .collect(),
        Requests::Sets { count, size, seed } => {
            let mut rng = Rng::new(seed);
            let mut sets = Vec::new();
            for at in 0..count {
                let mut set: BTreeSet<&str> = BTreeSet::new();
                while set.len() < size.min(names.len()) {
                    set.insert(rng.pick(names));
                }
                if options.shard.is_none_or(|shard| shard.takes(at)) {
                    sets.push(Job::Fresh(set.into_iter().map(str::to_owned).collect()));
                }
            }
            sets
        }
        // Each pair draws from a seed of its own, which the run's seed
        // draws in turn, so that a pair does not depend on how many names
        // the pairs before it drew, nor on which shard it fell in.
        Requests::Layered { count, seed, .. } => {
            let mut rng = Rng::new(seed);
            (0..count)
                .map(|at| (at, rng.next_u64()))
                .filter(|(at, _)| options.shard.is_none_or(|shard| shard.takes(*at)))
                .map(|(_, seed)| Job::Pair(seed))
                .collect()
        }
    }
}

/// The installed database of the root a layered pair's base makes: the
/// text of the records of the packages apk installs for `base` in an empty
/// root in `setting`, from `texts`, in the order apk installs them; or
/// `None` where apk refuses it.
///
/// # Errors
///
/// Fails where apk cannot be run, or installs a package `texts` holds no
/// record of.
fn base_root(
    apk: &Apk,
    setting: &Setting<'_>,
    texts: &BTreeMap<(String, String), String>,
    base: &[String],
    output: &Path,
) -> io::Result<Option<String>> {
    let answer = apk.add(setting, base, output)?;
    let Outcome::Changes(steps) = answer.outcome else {
        return Ok(None);
    };
    let mut installed = String::new();
    for step in &steps {
        let text = texts
            .get(&(step.name.clone(), step.version.clone()))
            .ok_or_else(|| {
                io::Error::other(format!(
                    "apk installs {} {}, which no index holds",
                    step.name, step.version
                ))
            })?;
        installed.push_str(text);
    }
    Ok(Some(installed))
}

/// What the workers of a sweep share.
struct Context<'a> {
    apk: &'a Apk,
    layout: &'a Layout,
    keys: &'a Path,
    repositories: &'a [Vec<Record>],
    resolver: &'a Resolver<'a>,
    names: &'a [&'a str],
    /// What layered pairs draw from, for a sweep of pairs.
    pairs: Option<Pairs<'a>>,
}

/// What a sweep's layered pairs draw from: the repositories a base is
/// installed from, their names, and each record's text by name and version.
struct Pairs<'a> {
    layout: &'a Layout,
    keys: PathBuf,
    names: &'a [&'a str],
    texts: BTreeMap<(String, String), String>,
    /// Whether a pair also asks for the base's package at a later version.
    upgrade: bool,
}

/// One worker's directory: the root apk resolves in, an empty root for a
/// layered pair's base, and the file apk's output goes to.
struct Work {
    _directory: TempDir,
    root: PathBuf,
    empty: PathBuf,
    output: PathBuf,
}

impl Work {
    fn new() -> io::Result<Work> {
        let directory = TempDir::new()?;
        let root = directory.path().join("root");
        let empty = directory.path().join("empty");
        empty_root(&root)?;
        empty_root(&empty)?;
        Ok(Work {
            root,
            empty,
            output: directory.path().join("apk-output"),
            _directory: directory,
        })
    }
}

impl Context<'_> {
    /// The result line of `job`, and its verdict.
    fn result(&self, job: &Job, work: &Work) -> io::Result<(Verdict, String)> {
        let (asked, theirs, ours) = match job {
            Job::Fresh(entries) => {
                let setting = self.layout.setting(&work.root, self.keys);
                let theirs = self.apk.add(&setting, entries, &work.output)?;
                let ours = ferrata_answer(self.resolver, entries)?;
                let asked = Asked {
                    base: Vec::new(),
                    entries: entries.clone(),
                };
                (asked, theirs, ours)
            }
            Job::Pair(seed) => self.pair(*seed, work)?,
        };
        let verdict = Verdict::of(&theirs, &ours);
        let (theirs, ours) = (written(&theirs), written(&ours));
        let ours = if verdict == Verdict::Same {
            "=".into()
        } else {
            ours
        };
        Ok((
            verdict,
            format!("{asked}\t{}\t{theirs}\t{ours}", verdict.word()),
        ))
    }

    /// Makes the layered pair `seed` draws, and gives apk's answer to it and
    /// ferrata's.
    fn pair(&self, seed: u64, work: &Work) -> io::Result<(Asked, Answer, Answer)> {
        let pairs = self
            .pairs
            .as_ref()
            .ok_or_else(|| io::Error::other("a layered pair in a sweep of no pairs"))?;
        let mut rng = Rng::new(seed);
        let requested = rng.pick(self.names);
        let setting = pairs.layout.setting(&work.empty, &pairs.keys);
        let mut made = None;
        for _ in 0..BASE_DRAWS {
            let name = rng.pick(pairs.names);
            let entries = [name.to_owned()];
            if let Some(root) = base_root(self.apk, &setting, &pairs.texts, &entries, &work.output)?
            {
                made = Some((name, root));
                break;
            }
        }
        let (name, installed) = made.ok_or_else(|| {
            io::Error::other(format!(
                "apk installs none of the {BASE_DRAWS} names seed {seed} draws for a base"
            ))
        })?;
        let records: Vec<Record> = index::read_installed(Dialect::V3_0, &installed)
            .map(|item| item.map(Located::into_record))
            .collect::<Result<_, _>>()
            .map_err(|error| io::Error::other(format!("{name}: the installed set: {error}")))?;
        let mut entries = vec![requested.to_owned()];
        if pairs.upgrade
            && let Some(old) = records.iter().find(|record| record.name() == name)
            && self
                .repositories
                .iter()
                .flatten()
                .any(|record| record.name() == name && record.version() > old.version())
        {
            entries.push(format!("{name}>{}", printed(old.version().as_bytes())));
        }
        let asked = Asked {
            base: vec![name.to_owned()],
            entries,
        };
        write_root(&work.root, &installed, &asked.base)?;
        let theirs = self.apk.add(
            &self.layout.setting(&work.root, self.keys),
            &asked.entries,
            &work.output,
        )?;
        let world = resolve::entries(&asked.base).map_err(io::Error::other)?;
        let resolver = resolve::resolver(self.repositories, &records, &world);
        let ours = ferrata_answer(&resolver, &asked.entries)?;
        Ok((asked, theirs, ours))
    }
}

/// ferrata's answer to `entries` from `resolver`, as [`resolve::written`]
/// writes it; a refusal where an entry does not read, as apk refuses it.
fn ferrata_answer(resolver: &Resolver<'_>, entries: &[String]) -> io::Result<Answer> {
    Ok(match resolve::request(entries) {
        Ok(parsed) => resolve::written(&resolver.resolve(&parsed)),
        Err(message) => Answer {
            apk: resolve::BANNER.into(),
            messages: vec![message],
            outcome: Outcome::Refused,
        },
    })
}

/// Runs a sweep over `layout` as `options` asks, writing the results to
/// `out` and each thousandth result's count to `progress`; returns the
/// counts.
///
/// # Errors
///
/// Fails where apk does not load the indexes cleanly, an index does not
/// read, apk cannot be run, or `out` cannot be written.
pub fn sweep(
    layout: &Layout,
    options: &Options,
    out: &mut impl Write,
    mut progress: impl FnMut(&str),
) -> io::Result<Counts> {
    let apk = match &options.apk {
        Some(program) => Apk::new(program)?,
        None => layout.apk()?,
    };
    let keys = layout.keys();
    let (repositories, stamps) = loaded(&apk, layout)?;
    let resolver = resolve::resolver(&repositories, &[], &[]);
    let names = names_of(&repositories);

    // A layered sweep's bases: from another fetched directory, verified and
    // stamped as the sweep's own repositories are, or from the sweep's own.
    let base_layout = options.base.as_deref().map(Layout::read).transpose()?;
    if let Some(base) = &base_layout
        && base.architecture != layout.architecture
    {
        return Err(io::Error::other(format!(
            "the base is for {}, the sweep for {}",
            base.architecture, layout.architecture
        )));
    }
    let (base_repositories, base_stamps) = match &base_layout {
        Some(base) => loaded(&apk, base)?,
        None => (Vec::new(), Vec::new()),
    };
    let base_names = names_of(&base_repositories);
    let pairs = match options.requests {
        Requests::Layered { upgrade, .. } => {
            let (layout, names) = match &base_layout {
                Some(base) => (base, base_names.as_slice()),
                None => (layout, names.as_slice()),
            };
            Some(Pairs {
                layout,
                keys: layout.keys(),
                names,
                texts: record_texts(layout)?,
                upgrade,
            })
        }
        _ => None,
    };
    let jobs = requests(&names, options);

    writeln!(out, "# ferrata {COMMIT}")?;
    writeln!(
        out,
        "# apk {} sha256:{}",
        apk.banner(),
        sha256(apk.program())?
    )?;
    writeln!(out, "# architecture {}", layout.architecture)?;
    for stamp in &stamps {
        writeln!(out, "# {stamp}")?;
    }
    for stamp in &base_stamps {
        writeln!(out, "# base {stamp}")?;
    }
    let shard = options
        .shard
        .map(|shard| format!(", {shard}"))
        .unwrap_or_default();
    match options.requests {
        Requests::Each => writeln!(out, "# requests: each name alone{shard}")?,
        Requests::Sets { count, size, seed } => writeln!(
            out,
            "# requests: {count} sets of {size} names from seed {seed}{shard}"
        )?,
        Requests::Layered {
            count,
            seed,
            upgrade,
        } => {
            let upgrade = if upgrade {
                ", the base's package asked at a later version where offered"
            } else {
                ""
            };
            writeln!(
                out,
                "# requests: {count} layered pairs from seed {seed}{upgrade}{shard}"
            )?
        }
    }

    let context = Context {
        apk: &apk,
        layout,
        keys: &keys,
        repositories: &repositories,
        resolver: &resolver,
        names: &names,
        pairs,
    };
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (sender, receiver) = mpsc::channel::<(usize, io::Result<(Verdict, String)>)>();
    let mut counts = Counts::default();
    let mut failure = None;
    std::thread::scope(|scope| -> io::Result<()> {
        for _ in 0..options.jobs.max(1) {
            let sender = sender.clone();
            let (context, jobs, next, stop) = (&context, &jobs, &next, &stop);
            scope.spawn(move || {
                let work = match Work::new() {
                    Ok(work) => work,
                    Err(error) => {
                        let _ = sender.send((usize::MAX, Err(error)));
                        return;
                    }
                };
                while !stop.load(Ordering::Relaxed) {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(at) else {
                        break;
                    };
                    if sender.send((at, context.result(job, &work))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        // The one writer: results arrive in any order and are written in
        // the order of the requests.
        let mut waiting: BTreeMap<usize, String> = BTreeMap::new();
        let mut written_up_to = 0;
        for (at, result) in receiver {
            match result {
                Ok((verdict, line)) => {
                    counts.add(verdict);
                    waiting.insert(at, line);
                }
                Err(error) => {
                    stop.store(true, Ordering::Relaxed);
                    failure.get_or_insert(error);
                    continue;
                }
            }
            while let Some(line) = waiting.remove(&written_up_to) {
                writeln!(out, "{line}")?;
                written_up_to += 1;
                if written_up_to % 1000 == 0 {
                    out.flush()?;
                    progress(&format!("{written_up_to} of {}", jobs.len()));
                }
            }
        }
        Ok(())
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    writeln!(out, "# {counts}")?;
    out.flush()?;
    Ok(counts)
}

/// The records of `layout`'s repositories and the lines that stamp them,
/// once apk has loaded its indexes, verifying each.
fn loaded(apk: &Apk, layout: &Layout) -> io::Result<(Vec<Vec<Record>>, Vec<String>)> {
    let descriptions = verified(apk, layout)?;
    repositories(layout, &descriptions)
}

/// Has `apk` load the indexes of `layout`'s repositories, which verifies
/// each with the layout's keys, and gives each one's description.
///
/// # Errors
///
/// Fails where apk does not load each cleanly, as [`Apk::update`] says.
fn verified(apk: &Apk, layout: &Layout) -> io::Result<Vec<String>> {
    let check = TempDir::new()?;
    let root = check.path().join("root");
    empty_root(&root)?;
    let keys = layout.keys();
    apk.update(&layout.setting(&root, &keys))
}

/// The scenario of the request `asked` of a sweep over `layout`, its base,
/// if it has one, installed from `base`'s repositories or from `layout`'s
/// where `base` is `None`, as `apk` installs it: every record of the
/// repositories, the root the base makes, and the request, for the reducer
/// to shrink. The indexes are read once `apk` has loaded them, verifying
/// each, as a sweep reads them. A record built for the layout's architecture is made one for
/// the architecture scenarios are resolved for, [`apk::ARCHITECTURE`], which
/// is the same package to apk.
///
/// # Errors
///
/// Fails where apk does not load an index cleanly, an index does not read,
/// apk cannot be run, or apk refuses the base.
pub fn case(
    layout: &Layout,
    base: Option<&Layout>,
    apk: &Apk,
    asked: &Asked,
) -> io::Result<Scenario> {
    let architecture = |records: Vec<format::Record>| -> Vec<format::Record> {
        records
            .into_iter()
            .map(|mut record| {
                for (letter, value) in &mut record.fields {
                    if *letter == 'A' && *value == layout.architecture {
                        apk::ARCHITECTURE.clone_into(value);
                    }
                }
                record
            })
            .collect()
    };
    verified(apk, layout)?;
    if let Some(base) = base {
        verified(apk, base)?;
    }
    let mut scenario = Scenario {
        statement: format!(
            "Made from a sweep over {}, for: {asked}",
            layout.urls.join(" ")
        ),
        request: asked.entries.clone(),
        ..Scenario::default()
    };
    for archive in layout.indexes() {
        let records = format::index_records(&index_text(&archive)?).map_err(io::Error::other)?;
        scenario
            .repositories
            .push(Repository::Records(architecture(records)));
    }
    if !asked.base.is_empty() {
        let from = base.unwrap_or(layout);
        let work = Work::new()?;
        let keys = from.keys();
        let texts = record_texts(from)?;
        let installed = base_root(
            apk,
            &from.setting(&work.empty, &keys),
            &texts,
            &asked.base,
            &work.output,
        )?
        .ok_or_else(|| {
            io::Error::other(format!("apk refuses the base {}", asked.base.join(" ")))
        })?;
        scenario.installed =
            architecture(format::index_records(&installed).map_err(io::Error::other)?);
        scenario.world = asked.base.clone();
    }
    Ok(scenario)
}

/// One request's result, read back from a results file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The request.
    pub request: String,
    /// How the answers compared.
    pub verdict: Verdict,
    /// apk's answer, as written.
    pub apk: String,
    /// ferrata's answer, as written, `=` replaced by apk's.
    pub ferrata: String,
}

/// A results file, read back.
#[derive(Debug, Clone, Default)]
pub struct Results {
    /// The lines that stamp it, without their `# `.
    pub stamps: Vec<String>,
    /// The results, in order.
    pub rows: Vec<Row>,
}

impl Results {
    /// Reads a results file.
    ///
    /// # Errors
    ///
    /// Fails on a line that is not a result.
    pub fn parse(text: &str) -> Result<Results, String> {
        let mut results = Results::default();
        for (number, line) in text.lines().enumerate() {
            if let Some(stamp) = line.strip_prefix("# ") {
                results.stamps.push(stamp.to_owned());
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let [request, verdict, apk, ferrata] = fields.as_slice() else {
                return Err(format!("line {}: not a result", number + 1));
            };
            let verdict = Verdict::from_word(verdict)
                .ok_or_else(|| format!("line {}: no verdict {verdict:?}", number + 1))?;
            results.rows.push(Row {
                request: (*request).to_owned(),
                verdict,
                apk: (*apk).to_owned(),
                ferrata: (if *ferrata == "=" { *apk } else { *ferrata }).to_owned(),
            });
        }
        Ok(results)
    }

    /// The shard of a run the results hold, as their request line names
    /// it, or `None` for a whole run.
    ///
    /// # Errors
    ///
    /// Fails where the request line names a shard that does not read.
    pub fn shard(&self) -> Result<Option<Shard>, String> {
        let shard = self
            .stamps
            .iter()
            .filter(|stamp| stamp.starts_with("requests:"))
            .find_map(|stamp| stamp.rsplit_once(", shard "));
        let Some((_, shard)) = shard else {
            return Ok(None);
        };
        shard
            .split_once(" of ")
            .and_then(|(index, count)| Shard::parse(&format!("{index}/{count}")))
            .map(Some)
            .ok_or_else(|| format!("not a shard: {shard:?}"))
    }

    /// The results of a run, from one file holding the whole run or from
    /// one file for each of its shards: each file must end in the count of
    /// its results, as a sweep that finished writes it; each shard of the
    /// run must be there, once; and the files' stamps must agree but for the
    /// shard and the counts. The merged results' request line names the
    /// shards they hold.
    ///
    /// # Errors
    ///
    /// Fails where a file does not read or was cut short, a whole run is
    /// merged with another file, a shard is missing or given twice, or the
    /// files come from different runs.
    pub fn merge(paths: &[PathBuf]) -> Result<Results, String> {
        let mut merged = Results::default();
        let mut first: Option<(PathBuf, Vec<String>)> = None;
        let mut shards: BTreeMap<usize, &PathBuf> = BTreeMap::new();
        let mut count: Option<(usize, &PathBuf)> = None;
        for path in paths {
            let at = |error: String| format!("{}: {error}", path.display());
            let text = std::fs::read_to_string(path).map_err(|e| at(e.to_string()))?;
            let results = Results::parse(&text).map_err(at)?;
            if results.stamps.last() != Some(&results.counts().to_string()) {
                return Err(at(
                    "it does not end in the count of its results, as a sweep that finished writes it"
                        .into(),
                ));
            }
            match results.shard().map_err(at)? {
                None if paths.len() > 1 => {
                    return Err(at("a whole run, which merges with no other file".into()));
                }
                None => {}
                Some(shard) => {
                    match count {
                        Some((other, first)) if other != shard.count => {
                            return Err(format!(
                                "{} is a shard of {other}, and {} one of {}",
                                first.display(),
                                path.display(),
                                shard.count
                            ));
                        }
                        Some(_) => {}
                        None => count = Some((shard.count, path)),
                    }
                    if let Some(other) = shards.insert(shard.index, path) {
                        return Err(format!(
                            "{} and {} are both {shard}",
                            other.display(),
                            path.display()
                        ));
                    }
                }
            }
            let stamps = results.identity();
            match &first {
                Some((path_first, first_stamps)) if *first_stamps != stamps => {
                    return Err(format!(
                        "{} and {} come from different runs:\n  {}\n  {}",
                        path_first.display(),
                        path.display(),
                        first_stamps.join("\n  "),
                        stamps.join("\n  ")
                    ));
                }
                Some(_) => {}
                None => {
                    merged.stamps = stamps.clone();
                    first = Some((path.clone(), stamps));
                }
            }
            merged.rows.extend(results.rows);
        }
        if let Some((count, _)) = count {
            let missing: Vec<String> = (1..=count)
                .filter(|index| !shards.contains_key(index))
                .map(|index| index.to_string())
                .collect();
            match missing.as_slice() {
                [] => {}
                [one] => return Err(format!("shard {one} of {count} is missing")),
                _ => {
                    return Err(format!(
                        "shards {} of {count} are missing",
                        missing.join(", ")
                    ));
                }
            }
            for stamp in &mut merged.stamps {
                if stamp.starts_with("requests:") {
                    stamp.push_str(&format!(", shards 1 to {count} of {count}"));
                }
            }
        }
        Ok(merged)
    }

    /// The stamps that say what produced the results: all but the request
    /// line's shard and the counts.
    fn identity(&self) -> Vec<String> {
        self.stamps
            .iter()
            .filter(|stamp| !stamp.contains(" requests:"))
            .map(|stamp| match stamp.split_once(", shard ") {
                Some((before, _)) if stamp.starts_with("requests:") => before.to_owned(),
                _ => stamp.clone(),
            })
            .collect()
    }

    /// How many results had each verdict.
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for row in &self.rows {
            counts.add(row.verdict);
        }
        counts
    }
}

/// Where a request's answers changed between two runs, the `before` and the
/// `after` results of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change<'a> {
    /// The results before.
    pub before: &'a Row,
    /// The results after.
    pub after: &'a Row,
}

/// The requests both runs made whose apk answer differs, and those whose
/// ferrata answer differs, in the order of `after`.
pub fn changes<'a>(before: &'a Results, after: &'a Results) -> (Vec<Change<'a>>, Vec<Change<'a>>) {
    let earlier: BTreeMap<&str, &Row> = before
        .rows
        .iter()
        .map(|row| (row.request.as_str(), row))
        .collect();
    let mut apk = Vec::new();
    let mut ferrata = Vec::new();
    for row in &after.rows {
        if let Some(&old) = earlier.get(row.request.as_str()) {
            let change = Change {
                before: old,
                after: row,
            };
            if old.apk != row.apk {
                apk.push(change.clone());
            }
            if old.ferrata != row.ferrata {
                ferrata.push(change);
            }
        }
    }
    (apk, ferrata)
}

/// The path a shard's results go to beside `path`: `path` itself without a
/// shard.
pub fn shard_path(path: &Path, shard: Option<Shard>) -> PathBuf {
    match shard {
        None => path.to_owned(),
        Some(shard) => {
            let mut name = path.file_stem().unwrap_or_default().to_os_string();
            name.push(format!("-{}-of-{}", shard.index, shard.count));
            if let Some(extension) = path.extension() {
                name.push(".");
                name.push(extension);
            }
            path.with_file_name(name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Step;

    fn installs(names: &[&str]) -> Answer {
        Answer {
            apk: "apk".into(),
            messages: Vec::new(),
            outcome: Outcome::Changes(
                names
                    .iter()
                    .map(|name| Step {
                        action: Action::Install,
                        name: (*name).into(),
                        from: None,
                        version: "1".into(),
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn verdicts_tell_order_from_set() {
        let refused = Answer {
            apk: "apk".into(),
            messages: Vec::new(),
            outcome: Outcome::Refused,
        };
        assert_eq!(
            Verdict::of(&installs(&["a", "b"]), &installs(&["a", "b"])),
            Verdict::Same
        );
        assert_eq!(
            Verdict::of(&installs(&["a", "b"]), &installs(&["b", "a"])),
            Verdict::Order
        );
        assert_eq!(
            Verdict::of(&installs(&["a", "b"]), &installs(&["a"])),
            Verdict::Set
        );
        // A step made twice is not the same as made once.
        assert_eq!(
            Verdict::of(&installs(&["a", "a"]), &installs(&["a"])),
            Verdict::Set
        );
        assert_eq!(Verdict::of(&refused, &installs(&["a"])), Verdict::Refusal);
        assert_eq!(Verdict::of(&refused, &refused), Verdict::Same);
    }

    #[test]
    fn a_change_is_written_with_the_version_it_leaves() {
        let step = |action, from: Option<&str>, version: &str| Step {
            action,
            name: "lib".into(),
            from: from.map(str::to_owned),
            version: version.into(),
        };
        let answer = |steps| Answer {
            apk: "apk".into(),
            messages: Vec::new(),
            outcome: Outcome::Changes(steps),
        };
        let from_one = answer(vec![step(Action::Upgrade, Some("1"), "3")]);
        let from_two = answer(vec![step(Action::Upgrade, Some("2"), "3")]);
        assert_eq!(written(&from_one), "Upgrade:lib=1->3");
        // Two answers whose steps differ only where they upgrade from
        // differ as written.
        assert_eq!(Verdict::of(&from_one, &from_two), Verdict::Set);
        assert_ne!(written(&from_one), written(&from_two));
        let removed = answer(vec![
            step(Action::Install, None, "1"),
            step(Action::Remove, None, "2"),
        ]);
        assert_eq!(written(&removed), "lib=1 Remove:lib=2");
    }

    #[test]
    fn a_shard_takes_its_share_of_names() {
        let shards: Vec<Shard> = (1..=4).map(|index| Shard { index, count: 4 }).collect();
        for name in ["musl", "busybox", "so:libc.musl-x86_64.so.1"] {
            assert_eq!(shards.iter().filter(|shard| shard.holds(name)).count(), 1);
        }
        assert_eq!(Shard::parse("2/4"), Some(shards[1]));
        assert_eq!(Shard::parse("0/4"), None);
        assert_eq!(Shard::parse("5/4"), None);
    }

    #[test]
    fn results_read_back() {
        let text = "# ferrata abc\n# requests: each name alone, shard 1 of 2\n\
                    a\tsame\ta=1\t=\nb\torder\ta=1 b=1\tb=1 a=1\n# 2 requests: 1 same, 1 order, 0 set, 0 refusal\n";
        let results = Results::parse(text).unwrap();
        assert_eq!(results.rows[0].ferrata, "a=1");
        assert_eq!(results.rows[1].verdict, Verdict::Order);
        assert_eq!(
            results.identity(),
            ["ferrata abc", "requests: each name alone"]
        );
        assert_eq!(
            results.counts().to_string(),
            "2 requests: 1 same, 1 order, 0 set, 0 refusal"
        );
    }

    #[test]
    fn sets_are_drawn_again_from_their_seed_and_divided_among_shards() {
        let names = ["a", "b", "c", "d", "e", "f"];
        let options = |shard| Options {
            apk: None,
            requests: Requests::Sets {
                count: 10,
                size: 3,
                seed: 7,
            },
            shard,
            jobs: 1,
            base: None,
        };
        let all = requests(&names, &options(None));
        assert_eq!(all, requests(&names, &options(None)));
        assert!(
            all.iter()
                .all(|job| matches!(job, Job::Fresh(set) if set.len() == 3))
        );
        let mut divided: Vec<Job> = (1..=3)
            .flat_map(|index| requests(&names, &options(Some(Shard { index, count: 3 }))))
            .collect();
        let mut all = all;
        divided.sort_by_key(|job| format!("{job:?}"));
        all.sort_by_key(|job| format!("{job:?}"));
        assert_eq!(divided, all);
    }

    #[test]
    fn layered_pairs_are_drawn_again_from_their_seed_and_divided_among_shards() {
        let options = |shard| Options {
            apk: None,
            requests: Requests::Layered {
                count: 10,
                seed: 7,
                upgrade: false,
            },
            shard,
            jobs: 1,
            base: None,
        };
        let all = requests(&["a", "b"], &options(None));
        assert_eq!(all, requests(&["a", "b", "c"], &options(None)));
        assert_eq!(all.len(), 10);
        let divided: Vec<Job> = (1..=3)
            .flat_map(|index| requests(&[], &options(Some(Shard { index, count: 3 }))))
            .collect();
        assert_eq!(divided.len(), 10);
        assert!(divided.iter().all(|job| all.contains(job)));
    }

    #[test]
    fn a_layered_request_reads_back() {
        for (text, base, entries) in [
            ("a", &[][..], &["a"][..]),
            ("a b", &[], &["a", "b"]),
            ("a > b", &["a"], &["b"]),
            ("a > b a>1.0-r0", &["a"], &["b", "a>1.0-r0"]),
        ] {
            let asked = Asked::parse(text);
            assert_eq!(asked.base, base);
            assert_eq!(asked.entries, entries);
            assert_eq!(asked.to_string(), text);
        }
    }

    #[test]
    fn a_run_merges_from_each_of_its_shards_once() {
        let directory = TempDir::new().unwrap();
        let file = |name: &str, shard: &str, rows: &[&str], counted: bool| {
            let mut text = format!("# ferrata abc\n# requests: each name alone{shard}\n");
            let mut counts = Counts::default();
            for row in rows {
                text.push_str(&format!("{row}\tsame\t{row}=1\t=\n"));
                counts.add(Verdict::Same);
            }
            if counted {
                text.push_str(&format!("# {counts}\n"));
            }
            let path = directory.path().join(name);
            std::fs::write(&path, text).unwrap();
            path
        };
        let one = file("1", ", shard 1 of 2", &["a"], true);
        let two = file("2", ", shard 2 of 2", &["b", "c"], true);
        let merged = Results::merge(&[one.clone(), two.clone()]).unwrap();
        assert_eq!(merged.rows.len(), 3);
        assert_eq!(
            merged.stamps,
            [
                "ferrata abc",
                "requests: each name alone, shards 1 to 2 of 2"
            ]
        );
        let whole = file("whole", "", &["a", "b"], true);
        assert_eq!(
            Results::merge(std::slice::from_ref(&whole))
                .unwrap()
                .rows
                .len(),
            2
        );

        let refused = |paths: &[PathBuf], says: &str| {
            let error = Results::merge(paths).unwrap_err();
            assert!(error.contains(says), "{error}");
        };
        refused(&[one.clone(), one.clone()], "are both shard 1 of 2");
        refused(std::slice::from_ref(&one), "shard 2 of 2 is missing");
        let of_four = file("of-four", ", shard 1 of 4", &["a"], true);
        refused(
            std::slice::from_ref(&of_four),
            "shards 2, 3, 4 of 4 are missing",
        );
        refused(&[one.clone(), of_four], "is a shard of 2, and");
        refused(&[whole, two.clone()], "a whole run");
        let cut = file("cut", ", shard 2 of 2", &["b"], false);
        refused(&[one.clone(), cut], "does not end in the count");
        let miscounted = file("miscounted", ", shard 2 of 2", &["b"], true);
        let text = std::fs::read_to_string(&miscounted).unwrap();
        std::fs::write(&miscounted, text.replace("1 requests", "2 requests")).unwrap();
        refused(&[one, miscounted], "does not end in the count");
    }

    #[test]
    fn a_shard_writes_beside_the_whole() {
        let shard = Some(Shard { index: 3, count: 8 });
        assert_eq!(
            shard_path(Path::new("out/v3.24.txt"), shard),
            Path::new("out/v3.24-3-of-8.txt")
        );
        assert_eq!(
            shard_path(Path::new("out/v3.24.txt"), None),
            Path::new("out/v3.24.txt")
        );
    }
}
