//! ferrata's reading and ordering of versions, held to apk's.
//!
//! `versions/ordered.txt` and `versions/refused.txt` record apk.static's
//! verdicts, and every run holds ferrata to them. With `FERRATA_APK_STATIC`
//! naming an `apk.static` binary, the live test asks it again: it checks the
//! recorded verdicts are still that binary's, and compares the two over
//! version strings made at random. `versions/README.md` says where the
//! recorded strings came from.

#[path = "versions/generate.rs"]
mod generate;

use std::cmp::Ordering;
use std::path::Path;
use std::process::Command;

use ferrata::Dialect;
use ferrata::version::Version;

fn read(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/versions")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The lines of a vector file that are not comments.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|line| !line.starts_with('#'))
}

/// `ordered.txt`: each version, and how it compares with the one before it.
fn ordered() -> Vec<(Ordering, String)> {
    let text = read("ordered.txt");
    lines(&text)
        .map(|line| {
            let (relation, version) = line
                .split_once(' ')
                .unwrap_or_else(|| panic!("{line:?} has no relation"));
            let relation = match relation {
                "<" => Ordering::Less,
                "=" => Ordering::Equal,
                _ => panic!("{line:?} has an unknown relation"),
            };
            (relation, version.to_owned())
        })
        .collect()
}

fn refused() -> Vec<String> {
    lines(&read("refused.txt")).map(str::to_owned).collect()
}

fn version(text: &str) -> Version {
    Dialect::V3_0
        .version(text)
        .unwrap_or_else(|error| panic!("{text:?}: {error}"))
}

#[test]
fn ferrata_orders_the_recorded_versions_as_apk_does() {
    let ordered = ordered();
    let versions: Vec<Version> = ordered.iter().map(|(_, text)| version(text)).collect();
    for (pair, (relation, _)) in versions.windows(2).zip(&ordered[1..]) {
        assert_eq!(
            pair[0].cmp(&pair[1]),
            *relation,
            "{} against {}",
            pair[0],
            pair[1],
        );
    }
    // The relations are adjacent ones, and apk's ordering is total, so a
    // sort that agrees on every adjacent pair agrees on every pair.
    let mut sorted = versions.clone();
    sorted.sort();
    assert!(sorted.iter().zip(&versions).all(|(a, b)| a.cmp(b).is_eq()));
    assert_eq!(versions.len(), 5_191);
}

#[test]
fn ferrata_refuses_what_apk_refuses() {
    let refused = refused();
    for text in &refused {
        assert!(
            Dialect::V3_0.version(text).is_err(),
            "{text:?} was read as a version"
        );
    }
    assert_eq!(refused.len(), 741);
}

/// The `apk.static` binary to hold ferrata to, if one is named.
fn apk() -> Option<String> {
    let apk = std::env::var("FERRATA_APK_STATIC").ok()?;
    let output = Command::new(&apk).arg("--version").output().unwrap();
    let banner = String::from_utf8_lossy(&output.stdout);
    eprintln!("comparing with {}", banner.trim());
    Some(apk)
}

/// The strings among `texts` that apk refuses as versions.
fn apk_refuses(apk: &str, texts: &[String]) -> Vec<String> {
    let mut refused = Vec::new();
    for chunk in texts.chunks(500) {
        let output = Command::new(apk)
            .args(["version", "-c", "--"])
            .args(chunk)
            .output()
            .unwrap();
        refused.extend(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(str::to_owned),
        );
    }
    refused
}

/// apk's comparison of each pair: `<`, `=` or `>`, as `apk version -t`
/// prints it.
fn apk_compares(apk: &str, pairs: &[(&str, &str)]) -> Vec<String> {
    let workers = std::thread::available_parallelism().map_or(4, usize::from);
    let share = pairs.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = pairs
            .chunks(share)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(a, b)| {
                            let output = Command::new(apk)
                                .args(["version", "-t", "--", a, b])
                                .output()
                                .unwrap();
                            String::from_utf8(output.stdout).unwrap().trim().to_owned()
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    })
}

fn symbol(order: Ordering) -> &'static str {
    match order {
        Ordering::Less => "<",
        Ordering::Equal => "=",
        Ordering::Greater => ">",
    }
}

/// The text of `version`, a valid one, which is ASCII.
fn text(version: &Version) -> &str {
    version.to_str().expect("a valid version is ASCII")
}

/// Every adjacent pair of `versions` on which apk disagrees with ferrata.
fn disagreements(apk: &str, versions: &[Version]) -> Vec<String> {
    let pairs: Vec<(&str, &str)> = versions
        .windows(2)
        .map(|pair| (text(&pair[0]), text(&pair[1])))
        .collect();
    let verdicts = apk_compares(apk, &pairs);
    versions
        .windows(2)
        .zip(verdicts)
        .filter_map(|(pair, verdict)| {
            let ours = symbol(pair[0].cmp(&pair[1]));
            (ours != verdict)
                .then(|| format!("{} {} ferrata {ours} apk {verdict}", pair[0], pair[1]))
        })
        .collect()
}

#[test]
fn apk_agrees() {
    let Some(apk) = apk() else {
        eprintln!("FERRATA_APK_STATIC is not set; nothing compared");
        return;
    };

    // The recorded verdicts are still apk's.
    let recorded: Vec<Version> = ordered().iter().map(|(_, text)| version(text)).collect();
    let differences = disagreements(&apk, &recorded);
    assert!(differences.is_empty(), "{differences:#?}");
    let mut refused = refused();
    refused.sort();
    let mut by_apk = apk_refuses(&apk, &refused);
    by_apk.sort();
    assert_eq!(by_apk, refused, "apk reads some of refused.txt");

    // Fresh strings from a seed, which a run can name to repeat another.
    let seed = std::env::var("FERRATA_SEED").map_or(1, |seed| seed.parse().unwrap());
    eprintln!("seed {seed}");
    let texts = generate::strings(seed, 4_000);
    let mut by_apk = apk_refuses(&apk, &texts);
    by_apk.sort();
    let mut ours: Vec<String> = texts
        .iter()
        .filter(|text| Dialect::V3_0.version(text).is_err())
        .cloned()
        .collect();
    ours.sort();
    assert_eq!(ours, by_apk, "ferrata and apk refuse different strings");

    let mut versions: Vec<Version> = texts
        .iter()
        .filter_map(|text| Dialect::V3_0.version(text).ok())
        .collect();
    versions.sort();
    let differences = disagreements(&apk, &versions);
    assert!(differences.is_empty(), "{differences:#?}");
    eprintln!(
        "{} strings, {} refused, {} versions ordered",
        texts.len(),
        by_apk.len(),
        versions.len(),
    );
}
