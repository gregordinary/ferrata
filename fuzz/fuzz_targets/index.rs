//! The record reader over arbitrary bytes: reading never panics, an index
//! record writes back as the bytes it was read from (without its blank
//! lines, and with a line feed added where the text ended without one),
//! spans run in order without overlapping, nothing is read after an error,
//! a record made of a record's fields is that record, and each entry it
//! depends on or is installed for, written as bytes, reads back as itself.
//!
//! The bytes are read both as an index and as an installed database. Where
//! every line is a field, they are also made into a record field by field,
//! which must give what the reader gives: the same record, the same error,
//! or, where the reader passes over fields no `P:` ends, no name.

#![no_main]

use ferrata::Dialect;
use ferrata::index::{self, ReadErrorKind};
use ferrata::record::Record;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &[u8]| {
    let lines: Vec<&[u8]> = text.split(|&byte| byte == b'\n').collect();
    let fields: Option<Vec<(u8, &[u8])>> = lines
        .iter()
        .take(
            lines
                .len()
                .saturating_sub(usize::from(text.ends_with(b"\n"))),
        )
        .map(|line| match line {
            [letter, b':', value @ ..] => Some((*letter, value)),
            _ => None,
        })
        .collect();
    if let Some(fields) = fields {
        let made = Record::from_fields(Dialect::V3_0, fields.iter().copied());
        match index::read(Dialect::V3_0, text).next() {
            Some(read) => assert_eq!(made, read.map(index::Located::into_record)),
            None => assert_eq!(
                made.map_err(|error| error.kind()),
                Err(ReadErrorKind::NoName)
            ),
        }
    }
    for installed in [false, true] {
        let records = if installed {
            index::read_installed(Dialect::V3_0, text)
        } else {
            index::read(Dialect::V3_0, text)
        };
        let mut end = 0;
        let mut failed = false;
        for item in records {
            assert!(!failed, "a record after an error");
            match item {
                Ok(located) => {
                    let span = located.span();
                    assert!(end <= span.start && span.start < span.end);
                    assert!(text[end..span.start].iter().all(|&byte| byte == b'\n'));
                    if !installed {
                        let mut read: Vec<u8> = text[span.clone()]
                            .split(|&byte| byte == b'\n')
                            .filter(|line| !line.is_empty())
                            .flat_map(|line| line.iter().copied().chain(*b"\n"))
                            .collect();
                        if read.is_empty() {
                            read.push(b'\n');
                        }
                        assert_eq!(located.record().as_bytes(), read);
                    }
                    end = span.end;
                    let record = located.record();
                    for dependency in record.depends().iter().chain(record.install_if()) {
                        dependency.admits(record.version());
                        // The empty name, read where a list begins with a
                        // space, has no spelling of its own.
                        if !dependency.name().as_bytes().is_empty() {
                            let written = dependency.to_bytes();
                            assert_eq!(Dialect::V3_0.dependency(&written).as_ref(), Ok(dependency));
                        }
                    }
                    let _ = record.to_string();
                    let made = Record::from_fields(Dialect::V3_0, record.fields());
                    assert_eq!(made.as_ref(), Ok(record));
                }
                Err(error) => {
                    assert!(error.record_line() <= error.line());
                    failed = true;
                }
            }
        }
    }
});
