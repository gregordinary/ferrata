//! The scenario file: one rule's repositories, installed set, request, and
//! the answer apk gave.
//!
//! A scenario is text in sections, each opened by a header on a line of its
//! own:
//!
//! ```text
//! [statement]
//! A name a real package carries is decided before a name provided at no
//! version, whichever a package lists first.
//!
//! [repository]
//! P:b
//! V:1.0-r0
//! D:v c
//!
//! P:c
//! V:1.0-r0
//!
//! [request]
//! b
//!
//! [answer apk-tools 3.0.9-r0]
//! install c 1.0-r0
//! install b 1.0-r0
//! ```
//!
//! - `[statement]` holds the rule in prose.
//! - `[repository]` holds package records in `APKINDEX` syntax, separated by
//!   blank lines. Each section is one repository, in the order apk is given
//!   them. A record without `C:` is given an identity derived from its name
//!   and version, so two records of one name and version are one package
//!   unless they state otherwise. `[repository verbatim]` holds the text of
//!   an index line by line, for a scenario about text apk cannot read: the
//!   section's lines, each ending in a line feed, up to its last line that
//!   is not blank. So it holds any text that ends in a line feed, does not
//!   end in a blank line, and has no line that would open a section (`[`,
//!   anything, `]`, but for a field such as `[:x]`); an empty section is
//!   the text of one line feed.
//! - `[installed]` holds the records of packages already installed, and
//!   `[world]` the world that installed them, one entry to a line. Without
//!   them, the root is empty.
//! - `[request]` holds what is asked of `apk add`, one argument to a line.
//! - `[answer ...]` holds apk's answer, the header naming the apk that gave
//!   it: one step to a line (`install`, `upgrade`, `downgrade`, `replace`,
//!   `reinstall` or `remove`, the name, and the version or the two
//!   versions), or
//!   `refused`; and apk's other output, each line after `| `.
//!
//! A line beginning `# ` is a comment, except inside a verbatim repository.

use std::fmt::{self, Write as _};

use crate::random::{Rng, hash};

/// One scenario.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scenario {
    /// The rule, in prose.
    pub statement: String,
    /// The repositories, in the order apk is given them.
    pub repositories: Vec<Repository>,
    /// The records of the packages installed before the request.
    pub installed: Vec<Record>,
    /// The world of the installed root.
    pub world: Vec<String>,
    /// The arguments to `apk add`.
    pub request: Vec<String>,
    /// apk's answer, once recorded.
    pub answer: Option<Answer>,
}

/// One repository's index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repository {
    /// Package records, each completed with an identity where it states none.
    Records(Vec<Record>),
    /// The text of the index, exactly as apk is to read it, within what the
    /// section can hold (see the [module documentation](self)).
    Verbatim(String),
}

/// A package record as the scenario writes it: its fields, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    /// Each field's letter and value.
    pub fields: Vec<(char, String)>,
}

impl Record {
    /// The value of the last field with `letter`.
    pub fn get(&self, letter: char) -> Option<&str> {
        self.fields
            .iter()
            .rev()
            .find(|(field, _)| *field == letter)
            .map(|(_, value)| value.as_str())
    }

    /// The package's name, or the empty name for a record without one.
    pub fn name(&self) -> &str {
        self.get('P').unwrap_or_default()
    }

    /// The package's version, or the empty version for a record without one.
    pub fn version(&self) -> &str {
        self.get('V').unwrap_or_default()
    }

    /// The record with an identity, where it states none: one derived from
    /// its name and version.
    pub fn completed(&self) -> Record {
        let mut record = self.clone();
        if record.get('C').is_none() {
            let identity = derived_identity(self.name(), self.version());
            record.fields.insert(0, ('C', identity));
        }
        record
    }

    /// The record's lines, each ending in a line feed.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for (letter, value) in &self.fields {
            let _ = writeln!(text, "{letter}:{value}");
        }
        text
    }
}

/// The text of a name's or a version's bytes as apk prints them: the bytes
/// themselves, read as UTF-8 with any sequence that is not replaced, rather
/// than the name's or the version's `Display`, which escapes a backslash.
/// A step of an answer names a package this way, and a request asks for one
/// so.
pub fn printed(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The identity a record of `name` at `version` is given when it states
/// none: `Q1` and twenty bytes derived from the two, so that distinct
/// packages have distinct digests.
pub fn derived_identity(name: &str, version: &str) -> String {
    // A hash of the two, spread over twenty bytes.
    let mut rng = Rng::new(hash(name.bytes().chain([0]).chain(version.bytes())));
    let mut digest = Vec::with_capacity(24);
    while digest.len() < 20 {
        digest.extend_from_slice(&rng.next_u64().to_be_bytes());
    }
    digest.truncate(20);
    format!("Q1{}", base64(&digest))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut group = [0u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let value = u32::from_be_bytes([0, group[0], group[1], group[2]]);
        for index in 0..4 {
            if index <= chunk.len() {
                let at = (value >> (18 - 6 * index)) & 0x3f;
                out.push(char::from(ALPHABET[at as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// apk's answer to a scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The apk that gave it, as `apk-tools 3.0.9-r0`.
    pub apk: String,
    /// What apk printed besides its steps, in order.
    pub messages: Vec<String>,
    /// What it did.
    pub outcome: Outcome,
}

/// The changeset apk carries out, or its refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The steps, in apk's order.
    Changes(Vec<Step>),
    /// apk refused the request.
    Refused,
}

/// One step of a changeset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Step {
    /// What is done to the package.
    pub action: Action,
    /// The package's name.
    pub name: String,
    /// The version installed before, for a step whose action
    /// [replaces](Action::replaces) one, and only for one.
    pub from: Option<String>,
    /// The version the step leaves installed, or the one it removes.
    pub version: String,
}

/// What a step does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    /// Installs a package not installed before.
    Install,
    /// Replaces an installed package with a later version.
    Upgrade,
    /// Replaces an installed package with an earlier version.
    Downgrade,
    /// Replaces an installed package with another of the same version.
    Replace,
    /// Installs again a package already installed.
    Reinstall,
    /// Removes an installed package.
    Remove,
}

impl Action {
    /// Whether a step of this action replaces an installed package with
    /// another of its name, and so names the version it replaces as well as
    /// the one it leaves: an upgrade, a downgrade and a replacement do.
    pub fn replaces(self) -> bool {
        matches!(self, Action::Upgrade | Action::Downgrade | Action::Replace)
    }

    fn word(self) -> &'static str {
        match self {
            Action::Install => "install",
            Action::Upgrade => "upgrade",
            Action::Downgrade => "downgrade",
            Action::Replace => "replace",
            Action::Reinstall => "reinstall",
            Action::Remove => "remove",
        }
    }

    fn from_word(word: &str) -> Option<Action> {
        Some(match word {
            "install" => Action::Install,
            "upgrade" => Action::Upgrade,
            "downgrade" => Action::Downgrade,
            "replace" => Action::Replace,
            "reinstall" => Action::Reinstall,
            "remove" => Action::Remove,
            _ => return None,
        })
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.action.word(), self.name)?;
        if let Some(from) = &self.from {
            write!(f, " {from}")?;
        }
        write!(f, " {}", self.version)
    }
}

impl Step {
    /// Reads a step as the answer section writes it.
    pub fn parse(line: &str) -> Option<Step> {
        let words: Vec<&str> = line.split(' ').collect();
        let action = Action::from_word(words.first()?)?;
        match (action.replaces(), words.as_slice()) {
            (false, [_, name, version]) => Some(Step {
                action,
                name: (*name).to_owned(),
                from: None,
                version: (*version).to_owned(),
            }),
            (true, [_, name, from, version]) => Some(Step {
                action,
                name: (*name).to_owned(),
                from: Some((*from).to_owned()),
                version: (*version).to_owned(),
            }),
            _ => None,
        }
    }
}

impl Answer {
    /// Reads answer lines: steps, `refused`, and `| ` messages. `apk` names
    /// the apk that gave them.
    pub fn parse_lines(apk: &str, text: &str) -> Result<Answer, String> {
        let mut messages = Vec::new();
        let mut steps = Vec::new();
        let mut refused = false;
        for line in text.lines() {
            if let Some(message) = line.strip_prefix("| ") {
                messages.push(message.to_owned());
            } else if line == "|" {
                messages.push(String::new());
            } else if line == "refused" {
                refused = true;
            } else if line.trim().is_empty() || is_comment(line) {
            } else {
                steps.push(Step::parse(line).ok_or_else(|| format!("not a step: {line:?}"))?);
            }
        }
        let outcome = match (refused, steps.is_empty()) {
            (true, true) => Outcome::Refused,
            (true, false) => return Err("an answer both refused and with steps".into()),
            (false, _) => Outcome::Changes(steps),
        };
        Ok(Answer {
            apk: apk.to_owned(),
            messages,
            outcome,
        })
    }

    /// Whether `other` is the same answer, whichever apk gave it: the same
    /// output besides its steps, and the same changes in the same order, or
    /// both refused. A recorded answer still holds where apk's answer now is
    /// the same answer.
    pub fn same_answer(&self, other: &Answer) -> bool {
        self.messages == other.messages && self.outcome == other.outcome
    }

    /// Whether `other` makes the same changes, in any order: both refuse,
    /// or the two make the same steps, each as many times.
    pub fn same_steps(&self, other: &Answer) -> bool {
        fn sorted(outcome: &Outcome) -> Option<Vec<&Step>> {
            match outcome {
                Outcome::Changes(steps) => {
                    let mut steps: Vec<&Step> = steps.iter().collect();
                    steps.sort();
                    Some(steps)
                }
                Outcome::Refused => None,
            }
        }
        sorted(&self.outcome) == sorted(&other.outcome)
    }

    /// The answer section: its header, naming the apk that gave the answer,
    /// and the answer's [lines](Answer::lines).
    pub fn section(&self) -> String {
        format!("[answer {}]\n{}", self.apk, self.lines())
    }

    /// The answer's lines, as the answer section holds them.
    pub fn lines(&self) -> String {
        let mut text = String::new();
        for message in &self.messages {
            if message.is_empty() {
                text.push_str("|\n");
            } else {
                let _ = writeln!(text, "| {message}");
            }
        }
        match &self.outcome {
            Outcome::Refused => text.push_str("refused\n"),
            Outcome::Changes(steps) => {
                for step in steps {
                    let _ = writeln!(text, "{step}");
                }
            }
        }
        text
    }
}

fn is_comment(line: &str) -> bool {
    line == "#" || line.starts_with("# ")
}

/// The words of `line` if it opens a section: `[`, words, `]`, and not a
/// field.
pub fn header(line: &str) -> Option<Vec<&str>> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    if line.as_bytes().get(1) == Some(&b':') {
        return None;
    }
    Some(inner.split(' ').filter(|word| !word.is_empty()).collect())
}

/// Splits record text into records at blank lines.
fn records(lines: &[&str]) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    let mut record = Record::default();
    for line in lines {
        if line.is_empty() {
            if !record.fields.is_empty() {
                records.push(std::mem::take(&mut record));
            }
            continue;
        }
        if is_comment(line) {
            continue;
        }
        let mut characters = line.chars();
        let (Some(letter), Some(':')) = (characters.next(), characters.next()) else {
            return Err(format!("not a field: {line:?}"));
        };
        record.fields.push((letter, characters.as_str().to_owned()));
    }
    if !record.fields.is_empty() {
        records.push(record);
    }
    Ok(records)
}

/// The records of an index's text, or of an installed database's, each as
/// its fields, as a `[repository]` or `[installed]` section reads them.
pub fn index_records(text: &str) -> Result<Vec<Record>, String> {
    records(&text.split('\n').collect::<Vec<&str>>())
}

/// The non-empty, non-comment lines of a list section.
fn entries(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| !line.is_empty() && !is_comment(line))
        .map(|line| (*line).to_owned())
        .collect()
}

impl Scenario {
    /// Reads a scenario file.
    pub fn parse(text: &str) -> Result<Scenario, String> {
        let mut scenario = Scenario::default();
        let lines: Vec<&str> = text.split('\n').collect();
        let mut at = 0;
        // Text before the first header may hold only comments.
        while at < lines.len() && header(lines[at]).is_none() {
            if !lines[at].is_empty() && !is_comment(lines[at]) {
                return Err(format!("line {}: text before any section", at + 1));
            }
            at += 1;
        }
        while at < lines.len() {
            let words = header(lines[at]).unwrap_or_default();
            let start = at + 1;
            at = start;
            while at < lines.len() && header(lines[at]).is_none() {
                at += 1;
            }
            let mut body = &lines[start..at];
            while let [rest @ .., ""] = body {
                body = rest;
            }
            let fail = |error: String| format!("section on line {}: {error}", start);
            match words.as_slice() {
                ["statement"] => {
                    let prose: Vec<&str> = body
                        .iter()
                        .copied()
                        .filter(|line| !is_comment(line))
                        .collect();
                    scenario.statement = prose.join("\n").trim().to_owned();
                }
                ["repository"] => scenario
                    .repositories
                    .push(Repository::Records(records(body).map_err(fail)?)),
                ["repository", "verbatim"] => {
                    let mut text = body.join("\n");
                    text.push('\n');
                    scenario.repositories.push(Repository::Verbatim(text));
                }
                ["installed"] => scenario.installed = records(body).map_err(fail)?,
                ["world"] => scenario.world = entries(body),
                ["request"] => scenario.request = entries(body),
                ["answer", apk @ ..] if !apk.is_empty() => {
                    scenario.answer =
                        Some(Answer::parse_lines(&apk.join(" "), &body.join("\n")).map_err(fail)?);
                }
                other => return Err(fail(format!("unknown section {other:?}"))),
            }
        }
        Ok(scenario)
    }
}

impl fmt::Display for Scenario {
    /// Writes the scenario file, which reads back as the same scenario.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut sections: Vec<String> = Vec::new();
        if !self.statement.is_empty() {
            sections.push(format!("[statement]\n{}\n", self.statement));
        }
        for repository in &self.repositories {
            sections.push(match repository {
                Repository::Records(records) => {
                    let records: Vec<String> = records.iter().map(Record::text).collect();
                    format!("[repository]\n{}", records.join("\n"))
                }
                Repository::Verbatim(text) => format!("[repository verbatim]\n{text}"),
            });
        }
        if !self.installed.is_empty() {
            let records: Vec<String> = self.installed.iter().map(Record::text).collect();
            sections.push(format!("[installed]\n{}", records.join("\n")));
        }
        if !self.world.is_empty() {
            sections.push(format!("[world]\n{}\n", self.world.join("\n")));
        }
        sections.push(format!("[request]\n{}\n", self.request.join("\n")));
        if let Some(answer) = &self.answer {
            sections.push(answer.section());
        }
        f.write_str(&sections.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verbatim_repository_holds_lines_up_to_the_last_that_is_not_blank() {
        let verbatim = |text: &str| {
            let scenario = Scenario::parse(text).unwrap();
            assert_eq!(Scenario::parse(&scenario.to_string()).unwrap(), scenario);
            match scenario.repositories.as_slice() {
                [Repository::Verbatim(text)] => text.clone(),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(
            verbatim("[repository verbatim]\nP:a\n# kept\n\nP:b\n\n\n[request]\na\n"),
            "P:a\n# kept\n\nP:b\n"
        );
        assert_eq!(
            verbatim("[repository verbatim]\nP:a\r\n[:x]\n[request]\na"),
            "P:a\r\n[:x]\n"
        );
        assert_eq!(verbatim("[repository verbatim]\n[request]\na\n"), "\n");
        // A line that would open a section ends the repository.
        assert!(Scenario::parse("[repository verbatim]\nP:a\n[x]\n[request]\na\n").is_err());
    }
}
