//! The `scenario` command: runs the behavior catalog's scenarios with
//! apk.static, records apk's answers, and reduces disagreements.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ferrata_scenario::apk::{Apk, materialize};
use ferrata_scenario::format::{Answer, Repository, Scenario, header, index_records};
use ferrata_scenario::random::Rng;
use ferrata_scenario::reduce::{Compare, Other, Reducer};
use ferrata_scenario::{differential, fetch, generate, sweep};

const HELP: &str = "\
scenario: the behavior catalog's tooling

  scenario run FILE
      Run FILE with apk and print apk's answer.
  scenario record FILE...
      Run each FILE with apk and write apk's answer into it. A file apk
      gives no answer for is left as it is and named, and the rest are
      recorded; the command then fails.
  scenario check PATH...
      Run every scenario in each PATH (a file, or a directory searched for
      *.scenario) and report each whose recorded answer is no longer apk's,
      and each apk gives no answer for; fails where there is either.
  scenario materialize FILE DIR
      Lay FILE out in DIR as apk and a second resolver are given it.
  scenario resolve PATH
      Print ferrata's answer to PATH: a scenario file, or a directory
      materialize laid one out in. As a second resolver for reduce, it is
      the command `scenario resolve`.
  scenario explain PATH [NAME]...
      Print why ferrata answers PATH as it does: for each package of the
      resolution, and each the root held that it drops, named NAME, or for
      every one, the chain of requirements behind it; or its refusal, and
      how that came about.
  scenario new -r INDEX... -q ENTRY... [-o OUT]
      Make a scenario from the text of real indexes (-r, in order) and a
      request (-q, in order).
  scenario generate SEED [-o OUT]
      Write the case made at random from SEED, as the property checks and
      the differential fuzzer make it.
  scenario fuzz [--seed S] [--count N] [--jobs J] [--reduce DIR]
      Resolve the cases N consecutive seeds from S make (1000 from 0 by
      default) with apk and with ferrata, J at a time, and report each on
      which they disagree, and each ferrata gives no answer to, which fails
      the command. With --reduce, shrink each disagreement, with ferrata as
      the second resolver, into DIR/seed-SEED.scenario.
  scenario prune FILE [-o OUT]
      Remove the records no resolution of FILE's request could reach, the
      reducer's first step.
  scenario reduce FILE --other COMMAND [--compare names|set|sequence]
                  [--keep NAME]... [--consistent] [-o OUT]
      Shrink FILE while apk and COMMAND disagree. COMMAND is split at spaces;
      the scenario's directory is appended to it. It prints its answer as a
      scenario's answer section writes one. With --consistent, keep the
      root one apk could have made: every installed package's dependencies
      and the world met by installed packages, and no `!` entry broken.

  scenario fetch DIR [--arch ARCH] [--key FILE]... [--release-apk] [URL...]
      Fetch into DIR the pinned apk.static and Alpine's keys, each checked
      against its pinned SHA-256, and the index of each repository URL for
      ARCH (x86_64), in the order given: by default, the pinned release's
      main and community. apk.static then loads the indexes, verifying each
      with the keys and any --key added. With --release-apk, also fetch the
      apk.static the repositories offer, where it is another version than
      the pinned one, into DIR/release/apk.static.
  scenario pins
      Print each package fetch pins, a line each: its file name, its
      SHA-256, and the URL the mirror serves it at.
  scenario sweep DIR [--apk FILE] [--shard I/N] [--jobs N]
                 [--sets COUNT --size K --seed S]
                 [--layered COUNT --seed S [--upgrade] [--base BASE]] [-o OUT]
      Resolve each name of DIR's repositories alone, or COUNT sets of K
      names drawn from seed S, with apk.static (DIR's own, once its SHA-256
      is checked against its pin, or FILE) and with
      ferrata, and write each result to OUT (standard output by default):
      only shard I of N with --shard, N at a time with --jobs. With
      --layered, resolve COUNT layered pairs drawn from seed S instead: a
      name apk installs into an empty root, from BASE's repositories (a
      directory fetch filled) or DIR's, and another name requested over
      that root; with --upgrade, the request also asks for the first name
      at a later version than the one installed, where DIR offers one.
  scenario case DIR [--base BASE] [--apk FILE] REQUEST [-o OUT]
      Write the scenario of one request of a sweep over DIR, as a results
      file writes the request: every record of DIR's repositories, the root
      a layered pair's base makes, installed from BASE's repositories or
      DIR's by apk.static (DIR's own, checked as sweep checks it, or FILE),
      and the request. prune and
      reduce shrink it.
  scenario summary FILE...
      Count the verdicts of a run, a results file or one for each of its
      shards, and list every request whose answers differ. Fails where a
      file was cut short, or a shard of the run is missing or given twice.
  scenario compare --before FILE... --after FILE...
      Report the requests two runs both made whose apk answer, or whose
      ferrata answer, changed between them.

run, record, check, fuzz and reduce ask the apk.static --apk names, or
else the one FERRATA_APK_STATIC names. sweep and case ask the one --apk
names, or else DIR's own, the pinned one. fetch runs the pinned one alone.
A word beginning with - that no command takes is an error.
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(arguments) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("scenario: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Takes `--apk PATH` out of `arguments`, or reads `FERRATA_APK_STATIC`.
fn apk(arguments: &mut Vec<String>) -> Result<Apk, String> {
    let named = match arguments.iter().position(|argument| argument == "--apk") {
        Some(at) => {
            arguments.remove(at);
            if at >= arguments.len() {
                return Err("--apk names no binary".into());
            }
            Some(arguments.remove(at))
        }
        None => None,
    };
    let apk = match named {
        Some(path) => Apk::new(path),
        None => Apk::from_env().ok_or("no apk.static: pass --apk or set FERRATA_APK_STATIC")?,
    };
    apk.map_err(|error| error.to_string())
}

/// The scenario file `path` names: the file itself, or the one in a
/// directory `scenario materialize` laid out.
fn scenario_file(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_dir() {
        path.join("scenario")
    } else {
        path.to_owned()
    }
}

fn read(path: &Path) -> Result<Scenario, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Scenario::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Every `*.scenario` file under `path`, or `path` itself if it is a file.
fn scenario_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_owned()]);
    }
    let mut files = Vec::new();
    let entries = fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            files.extend(scenario_files(&path)?);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "scenario")
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Runs the scenario file `path` with `apk`, and writes apk's answer into
/// it.
fn record(apk: &Apk, path: &Path) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let scenario = Scenario::parse(&text)?;
    let answer = apk.answer(&scenario).map_err(|e| e.to_string())?;
    fs::write(path, with_answer(&text, &answer)).map_err(|e| e.to_string())
}

/// `text` with its answer section, if any, replaced by `answer`.
fn with_answer(text: &str, answer: &Answer) -> String {
    // At line feeds alone: `str::lines` would take a carriage return before
    // one as part of the line ending, and a scenario can hold one on purpose.
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines
        .iter()
        .position(|line| header(line).is_some_and(|words| words.first() == Some(&"answer")));
    let mut kept: Vec<&str> = match start {
        Some(start) => {
            let end = lines[start + 1..]
                .iter()
                .position(|line| header(line).is_some())
                .map_or(lines.len(), |at| start + 1 + at);
            lines[..start]
                .iter()
                .chain(&lines[end..])
                .copied()
                .collect()
        }
        None => lines,
    };
    while kept.last().is_some_and(|line| line.is_empty()) {
        kept.pop();
    }
    format!("{}\n\n{}", kept.join("\n"), answer.section())
}

fn run(mut arguments: Vec<String>) -> Result<ExitCode, String> {
    let Some(command) = arguments.first().cloned() else {
        print!("{HELP}");
        return Ok(ExitCode::FAILURE);
    };
    arguments.remove(0);
    // Commands that take files, paths and names alone.
    let positional = [
        "run",
        "record",
        "check",
        "resolve",
        "explain",
        "materialize",
    ];
    if positional.contains(&command.as_str())
        && let Some(flag) = arguments
            .iter()
            .find(|word| word.starts_with('-') && *word != "--apk")
    {
        return Err(unknown_flag(flag));
    }
    match command.as_str() {
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(ExitCode::SUCCESS)
        }
        "run" => {
            let apk = apk(&mut arguments)?;
            let [file] = arguments.as_slice() else {
                return Err("run takes one file".into());
            };
            let answer = apk
                .answer(&read(Path::new(file))?)
                .map_err(|e| e.to_string())?;
            print!("{}", answer.section());
            Ok(ExitCode::SUCCESS)
        }
        "record" => {
            let apk = apk(&mut arguments)?;
            // Each file apk answers is recorded, whatever the others do.
            let mut failed = 0;
            for file in &arguments {
                match record(&apk, Path::new(file)) {
                    Ok(()) => eprintln!("{file}: recorded"),
                    Err(error) => {
                        failed += 1;
                        eprintln!("{file}: not recorded: {error}");
                    }
                }
            }
            if failed > 0 {
                return Err(format!(
                    "{failed} of {} files not recorded",
                    arguments.len()
                ));
            }
            Ok(ExitCode::SUCCESS)
        }
        "check" => {
            let apk = apk(&mut arguments)?;
            let (mut checked, mut differ, mut failed) = (0, 0, 0);
            for argument in &arguments {
                for path in scenario_files(Path::new(argument))? {
                    let answered = read(&path).and_then(|scenario| {
                        let answer = apk.answer(&scenario).map_err(|e| e.to_string())?;
                        Ok((scenario, answer))
                    });
                    let (scenario, answer) = match answered {
                        Ok(answered) => answered,
                        Err(error) => {
                            failed += 1;
                            println!("{}: no answer: {error}", path.display());
                            continue;
                        }
                    };
                    checked += 1;
                    let recorded = scenario.answer.as_ref();
                    if !recorded.is_some_and(|recorded| recorded.same_answer(&answer)) {
                        differ += 1;
                        println!("{}: apk now answers\n{}", path.display(), answer.lines());
                    }
                }
            }
            eprintln!(
                "{checked} scenarios checked with {}, {differ} differ; {failed} not answered",
                apk.banner()
            );
            Ok(if differ == 0 && failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "resolve" => {
            let [path] = arguments.as_slice() else {
                return Err("resolve takes a scenario file or directory".into());
            };
            let answer = ferrata_scenario::resolve::answer(&read(&scenario_file(path))?)?;
            print!("{}", answer.lines());
            Ok(ExitCode::SUCCESS)
        }
        "explain" => {
            let Some((path, names)) = arguments.split_first() else {
                return Err("explain takes a scenario file or directory, and names".into());
            };
            let scenario = read(&scenario_file(path))?;
            print!(
                "{}",
                ferrata_scenario::resolve::explanations(&scenario, names)?
            );
            Ok(ExitCode::SUCCESS)
        }
        "materialize" => {
            let [file, directory] = arguments.as_slice() else {
                return Err("materialize takes a file and a directory".into());
            };
            fs::create_dir_all(directory).map_err(|e| e.to_string())?;
            materialize(&read(Path::new(file))?, Path::new(directory))
                .map_err(|e| e.to_string())?;
            Ok(ExitCode::SUCCESS)
        }
        "new" => {
            let mut scenario = Scenario::default();
            let mut output = None;
            let mut words = arguments.into_iter();
            while let Some(flag) = words.next() {
                let value = words.next().ok_or(format!("{flag} needs a value"))?;
                match flag.as_str() {
                    "-r" => {
                        let text =
                            fs::read_to_string(&value).map_err(|e| format!("{value}: {e}"))?;
                        scenario
                            .repositories
                            .push(Repository::Records(index_records(&text)?));
                    }
                    "-q" => scenario.request.push(value),
                    "-o" => output = Some(value),
                    _ => return Err(format!("unknown flag {flag}")),
                }
            }
            scenario.statement = format!(
                "Made from real indexes, for: {}",
                scenario.request.join(" ")
            );
            match output {
                Some(path) => fs::write(&path, scenario.to_string()).map_err(|e| e.to_string())?,
                None => print!("{scenario}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        "generate" => {
            let (seed, output) = match arguments.as_slice() {
                [seed] => (seed, None),
                [seed, flag, output] if flag == "-o" => (seed, Some(output)),
                _ => return Err("generate takes a seed and, optionally, -o OUT".into()),
            };
            let seed: u64 = seed.parse().map_err(|_| format!("{seed} is not a seed"))?;
            let mut scenario = generate::scenario(&mut Rng::new(seed));
            scenario.statement = format!("Made at random, from seed {seed}.");
            match output {
                Some(path) => fs::write(path, scenario.to_string()).map_err(|e| e.to_string())?,
                None => print!("{scenario}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        "fuzz" => {
            let apk = apk(&mut arguments)?;
            let (mut seed, mut count) = (0u64, 1000u64);
            let mut jobs = std::thread::available_parallelism().map_or(1, usize::from);
            let mut reduce_into = None;
            let mut words = arguments.into_iter();
            while let Some(word) = words.next() {
                let mut value = || words.next().ok_or(format!("{word} needs a value"));
                match word.as_str() {
                    "--seed" => seed = number(&value()?)? as u64,
                    "--count" => count = number(&value()?)? as u64,
                    "--jobs" => jobs = number(&value()?)?,
                    "--reduce" => reduce_into = Some(PathBuf::from(value()?)),
                    _ => return Err(format!("unexpected {word}")),
                }
            }
            let searched = differential::search(&apk, seed..seed.saturating_add(count), jobs)
                .map_err(|e| e.to_string())?;
            let findings = searched.findings;
            eprintln!(
                "{count} cases from seed {seed}: apk and ferrata disagree on {}, and ferrata gives no answer to {}",
                findings.len(),
                searched.unanswered.len()
            );
            for (seed, error) in &searched.unanswered {
                println!("seed {seed}\n  ferrata gives no answer: {error}");
            }
            for finding in &findings {
                let apk = finding.case.answer.as_ref().map(|answer| answer.lines());
                println!(
                    "seed {}\n  apk:     {}\n  ferrata: {}",
                    finding.seed,
                    apk.unwrap_or_default().replace('\n', " ").trim_end(),
                    finding.ferrata.lines().replace('\n', " ").trim_end()
                );
            }
            if let Some(directory) = reduce_into {
                fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                let program = std::env::current_exe().map_err(|e| e.to_string())?;
                let other = Other::new(vec![program.display().to_string(), "resolve".into()]);
                let next = std::sync::atomic::AtomicUsize::new(0);
                let failures = std::sync::Mutex::new(Vec::new());
                std::thread::scope(|scope| {
                    for _ in 0..jobs.max(1) {
                        scope.spawn(|| {
                            while let Some(finding) = findings
                                .get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                            {
                                let path =
                                    directory.join(format!("seed-{}.scenario", finding.seed));
                                let written = finding
                                    .reduce(&apk, &other)
                                    .and_then(|reduced| fs::write(&path, reduced.to_string()));
                                if let Err(error) = written
                                    && let Ok(mut failures) = failures.lock()
                                {
                                    failures.push(format!("seed {}: {error}", finding.seed));
                                }
                            }
                        });
                    }
                });
                let failures = failures.into_inner().unwrap_or_default();
                if !failures.is_empty() {
                    return Err(failures.join("\n"));
                }
                eprintln!("reduced into {}", directory.display());
            }
            if !searched.unanswered.is_empty() {
                return Err(format!(
                    "ferrata gives no answer to {} cases",
                    searched.unanswered.len()
                ));
            }
            Ok(ExitCode::SUCCESS)
        }
        "prune" => {
            let (file, output) = match arguments.as_slice() {
                [file] => (file, None),
                [file, flag, output] if flag == "-o" => (file, Some(output)),
                _ => return Err("prune takes a file and, optionally, -o OUT".into()),
            };
            let pruned = ferrata_scenario::reduce::prune(&read(Path::new(file))?);
            match output {
                Some(path) => fs::write(path, pruned.to_string()).map_err(|e| e.to_string())?,
                None => print!("{pruned}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        "reduce" => {
            let apk = apk(&mut arguments)?;
            let mut file = None;
            let mut other = None;
            let mut compare = Compare::Names;
            let mut keep = Vec::new();
            let mut consistent = false;
            let mut output = None;
            let mut words = arguments.into_iter();
            while let Some(word) = words.next() {
                let mut value = || words.next().ok_or(format!("{word} needs a value"));
                match word.as_str() {
                    "--other" => {
                        other = Some(Other::new(value()?.split(' ').map(String::from).collect()))
                    }
                    "--compare" => {
                        compare = match value()?.as_str() {
                            "names" => Compare::Names,
                            "set" => Compare::Set,
                            "sequence" => Compare::Sequence,
                            other => return Err(format!("unknown comparison {other}")),
                        }
                    }
                    "--keep" => keep.push(value()?),
                    "--consistent" => consistent = true,
                    "-o" => output = Some(value()?),
                    _ if word.starts_with('-') => return Err(unknown_flag(&word)),
                    _ if file.is_none() => file = Some(word),
                    _ => return Err(format!("unexpected {word}")),
                }
            }
            let file = file.ok_or("reduce needs a file")?;
            let other = other.ok_or("reduce needs --other")?;
            let scenario = read(Path::new(&file))?;
            let mut reducer = Reducer::new(&apk, &other, compare, keep, |line| eprintln!("{line}"))
                .consistent(consistent);
            let mut reduced = reducer.reduce(&scenario).map_err(|e| e.to_string())?;
            eprintln!(
                "{} tests, {} failed in the second resolver",
                reducer.tests, reducer.failures
            );
            reduced.answer = Some(apk.answer(&reduced).map_err(|e| e.to_string())?);
            reduced.statement = format!("Reduced from {file}.");
            match output {
                Some(path) => fs::write(&path, reduced.to_string()).map_err(|e| e.to_string())?,
                None => print!("{reduced}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        "fetch" => {
            let mut directory = None;
            let mut architecture = ferrata_scenario::apk::ARCHITECTURE.to_owned();
            let mut keys = Vec::new();
            let mut release_apk = false;
            let mut urls = Vec::new();
            let mut words = arguments.into_iter();
            while let Some(word) = words.next() {
                let mut value = || words.next().ok_or(format!("{word} needs a value"));
                match word.as_str() {
                    "--arch" => architecture = value()?,
                    "--key" => keys.push(PathBuf::from(value()?)),
                    "--release-apk" => release_apk = true,
                    _ if word.starts_with('-') => return Err(unknown_flag(&word)),
                    _ if directory.is_none() => directory = Some(PathBuf::from(word)),
                    _ => urls.push(word),
                }
            }
            let directory = directory.ok_or("fetch needs a directory")?;
            if urls.is_empty() {
                urls = fetch::default_repositories();
            }
            fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            let fetched = fetch::fetch(&directory, &architecture, &urls, &keys, release_apk)
                .map_err(|e| e.to_string())?;
            println!(
                "apk.static {} {} (pinned)",
                fetch::APK_STATIC.release,
                fetch::APK_STATIC.version
            );
            for (url, description) in urls.iter().zip(&fetched.descriptions) {
                println!("repository {url} {description}");
            }
            match &fetched.offered {
                Some(version) => println!("offered {} {version}", fetch::APK_STATIC.name),
                None => println!("offered no {}", fetch::APK_STATIC.name),
            }
            if let Some((binary, package)) = &fetched.release_apk {
                let digest = |path| fetch::sha256(path).map_err(|e| e.to_string());
                println!(
                    "offered apk.static in {} sha256:{}",
                    binary.display(),
                    digest(binary)?
                );
                println!(
                    "offered package {} sha256:{}",
                    package.display(),
                    digest(package)?
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        "pins" => {
            if !arguments.is_empty() {
                return Err("pins takes no arguments".into());
            }
            for pin in [fetch::APK_STATIC, fetch::KEYS] {
                println!("{} {} {}", pin.file_name(), pin.sha256, pin.url());
            }
            Ok(ExitCode::SUCCESS)
        }
        "sweep" => {
            let mut directory = None;
            let mut options = sweep::Options {
                apk: None,
                requests: sweep::Requests::Each,
                shard: None,
                jobs: std::thread::available_parallelism().map_or(1, usize::from),
                base: None,
            };
            let (mut count, mut size, mut seed) = (None, 5, 1);
            let (mut layered, mut upgrade) = (None, false);
            let mut output = None;
            let mut words = arguments.into_iter();
            while let Some(word) = words.next() {
                let mut value = || words.next().ok_or(format!("{word} needs a value"));
                match word.as_str() {
                    "--apk" => options.apk = Some(PathBuf::from(value()?)),
                    "--shard" => {
                        let text = value()?;
                        options.shard =
                            Some(sweep::Shard::parse(&text).ok_or(format!("{text} is not I/N"))?);
                    }
                    "--jobs" => options.jobs = number(&value()?)?,
                    "--sets" => count = Some(number(&value()?)?),
                    "--size" => size = number(&value()?)?,
                    "--seed" => seed = number(&value()?)? as u64,
                    "--layered" => layered = Some(number(&value()?)?),
                    "--upgrade" => upgrade = true,
                    "--base" => options.base = Some(PathBuf::from(value()?)),
                    "-o" => output = Some(PathBuf::from(value()?)),
                    _ if word.starts_with('-') => return Err(unknown_flag(&word)),
                    _ if directory.is_none() => directory = Some(PathBuf::from(word)),
                    _ => return Err(format!("unexpected {word}")),
                }
            }
            options.requests = match (count, layered) {
                (Some(_), Some(_)) => return Err("--sets and --layered exclude each other".into()),
                (Some(count), None) => sweep::Requests::Sets { count, size, seed },
                (None, Some(count)) => sweep::Requests::Layered {
                    count,
                    seed,
                    upgrade,
                },
                (None, None) => sweep::Requests::Each,
            };
            if layered.is_none() && (upgrade || options.base.is_some()) {
                return Err("--upgrade and --base need --layered".into());
            }
            let directory = directory.ok_or("sweep needs a directory")?;
            let layout = fetch::Layout::read(&directory).map_err(|e| e.to_string())?;
            let progress = |line: &str| eprintln!("{line}");
            let counts = match output {
                Some(path) => {
                    let file =
                        fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    let mut out = std::io::BufWriter::new(file);
                    sweep::sweep(&layout, &options, &mut out, progress)
                }
                None => sweep::sweep(&layout, &options, &mut std::io::stdout().lock(), progress),
            }
            .map_err(|e| e.to_string())?;
            eprintln!("{counts}");
            Ok(ExitCode::SUCCESS)
        }
        "case" => {
            let mut directory = None;
            let mut base = None;
            let mut program = None;
            let mut asked = None;
            let mut output = None;
            let mut words = arguments.into_iter();
            while let Some(word) = words.next() {
                let mut value = || words.next().ok_or(format!("{word} needs a value"));
                match word.as_str() {
                    "--base" => base = Some(PathBuf::from(value()?)),
                    "--apk" => program = Some(PathBuf::from(value()?)),
                    "-o" => output = Some(PathBuf::from(value()?)),
                    _ if word.starts_with('-') => return Err(unknown_flag(&word)),
                    _ if directory.is_none() => directory = Some(PathBuf::from(word)),
                    _ if asked.is_none() => asked = Some(sweep::Asked::parse(&word)),
                    _ => return Err(format!("unexpected {word}")),
                }
            }
            let (Some(directory), Some(asked)) = (directory, asked) else {
                return Err("case takes a directory and a request".into());
            };
            let layout = fetch::Layout::read(&directory).map_err(|e| e.to_string())?;
            let base = base
                .map(|base| fetch::Layout::read(&base))
                .transpose()
                .map_err(|e| e.to_string())?;
            let apk = match program {
                Some(program) => Apk::new(program),
                None => layout.apk(),
            }
            .map_err(|e| e.to_string())?;
            let scenario =
                sweep::case(&layout, base.as_ref(), &apk, &asked).map_err(|e| e.to_string())?;
            match output {
                Some(path) => fs::write(&path, scenario.to_string()).map_err(|e| e.to_string())?,
                None => print!("{scenario}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        "summary" => {
            let paths: Vec<PathBuf> = arguments.iter().map(PathBuf::from).collect();
            if paths.is_empty() {
                return Err("summary takes results files".into());
            }
            let results = sweep::Results::merge(&paths)?;
            for stamp in &results.stamps {
                println!("# {stamp}");
            }
            println!("# {}", results.counts());
            for row in results
                .rows
                .iter()
                .filter(|row| row.verdict != sweep::Verdict::Same)
            {
                println!(
                    "{}	{}
  apk:     {}
  ferrata: {}",
                    row.request,
                    row.verdict.word(),
                    row.apk,
                    row.ferrata
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        "compare" => {
            let (mut before, mut after) = (Vec::new(), Vec::new());
            let mut side = None;
            for word in arguments {
                match word.as_str() {
                    "--before" => side = Some(false),
                    "--after" => side = Some(true),
                    _ => match side {
                        Some(false) => before.push(PathBuf::from(word)),
                        Some(true) => after.push(PathBuf::from(word)),
                        None => return Err(format!("{word} is neither --before nor --after")),
                    },
                }
            }
            if before.is_empty() || after.is_empty() {
                return Err("compare takes --before FILE... --after FILE...".into());
            }
            let (before, after) = (
                sweep::Results::merge(&before)?,
                sweep::Results::merge(&after)?,
            );
            for (label, results) in [("before", &before), ("after", &after)] {
                println!("# {label}: {}", results.counts());
                for stamp in &results.stamps {
                    println!("#   {stamp}");
                }
            }
            let (apk, ferrata) = sweep::changes(&before, &after);
            for (side, changes) in [("apk", &apk), ("ferrata", &ferrata)] {
                println!("# {} requests whose {side} answer changed", changes.len());
                for change in changes {
                    let (old, new) = match side {
                        "apk" => (&change.before.apk, &change.after.apk),
                        _ => (&change.before.ferrata, &change.after.ferrata),
                    };
                    println!(
                        "{}	{} -> {}
  before: {old}
  after:  {new}",
                        change.after.request,
                        change.before.verdict.word(),
                        change.after.verdict.word()
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(format!("unknown command {command}; see scenario help")),
    }
}

/// The error for a flag the command does not take.
fn unknown_flag(flag: &str) -> String {
    format!("unknown flag {flag}; see scenario help")
}

/// `text` as a number.
fn number(text: &str) -> Result<usize, String> {
    text.parse().map_err(|_| format!("{text} is not a number"))
}
