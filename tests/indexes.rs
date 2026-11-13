//! The reader over real indexes, which are not vendored.
//!
//! With `FERRATA_INDEXES` naming a directory, every file in it is read as an
//! `APKINDEX` (decompressed and taken out of its archive), as bytes: each
//! must read to its end, each record must write back as the bytes it was
//! read from (without its blank lines, and with a line feed added where the
//! text ended without one), a record made of its fields must be the record
//! read, each entry it depends on or is installed for must read back from
//! the bytes it is written as, and the records' spans must account for the
//! whole text but the blank lines between them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ferrata::Dialect;
use ferrata::index;
use ferrata::record::{Flaw, Record};

#[test]
fn every_index_reads_and_writes_back() {
    let Ok(directory) = std::env::var("FERRATA_INDEXES") else {
        eprintln!("FERRATA_INDEXES is not set; nothing read");
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();

    let mut records = 0;
    let mut invalid_versions = 0;
    let mut flaws = BTreeMap::new();
    for path in &paths {
        let text = std::fs::read(path).unwrap();
        let mut covered = 0;
        for item in index::read(Dialect::V3_0, &text) {
            let located = item.unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let span = located.span();
            assert!(
                text[covered..span.start].iter().all(|&byte| byte == b'\n'),
                "{}: text between records at line {}",
                path.display(),
                located.line(),
            );
            let read: Vec<u8> = text[span.clone()]
                .split(|&byte| byte == b'\n')
                .filter(|line| !line.is_empty())
                .flat_map(|line| line.iter().copied().chain(*b"\n"))
                .collect();
            assert_eq!(
                located.record().as_bytes(),
                read,
                "{}: line {}",
                path.display(),
                located.line(),
            );
            assert_eq!(
                Record::from_fields(Dialect::V3_0, located.record().fields()).as_ref(),
                Ok(located.record()),
                "{}: line {}",
                path.display(),
                located.line(),
            );
            let record = located.record();
            for entry in record.depends().iter().chain(record.install_if()) {
                if !entry.name().as_bytes().is_empty() {
                    assert_eq!(
                        Dialect::V3_0.dependency(entry.to_bytes()).as_ref(),
                        Ok(entry),
                        "{}: line {}",
                        path.display(),
                        located.line(),
                    );
                }
            }
            covered = span.end;
            records += 1;
            invalid_versions += usize::from(!located.record().version().is_valid());
            for flaw in located.record().flaws() {
                let kind = match flaw {
                    Flaw::UnknownField(_) => "an unknown field",
                    Flaw::Depends(_) => "a D: entry that does not read",
                    Flaw::Provides => "a p: entry that does not read",
                    Flaw::InstallIf(_) => "an i: entry that does not read",
                    _ => "another flaw",
                };
                *flaws.entry(kind).or_insert(0) += 1;
            }
        }
        assert!(text[covered..].iter().all(|&byte| byte == b'\n'));
    }
    eprintln!(
        "{} indexes, {records} records, {invalid_versions} versions outside the grammar",
        paths.len()
    );
    for (flaw, count) in &flaws {
        eprintln!("{count:>8}  {flaw}");
    }
    assert!(!paths.is_empty());
}
