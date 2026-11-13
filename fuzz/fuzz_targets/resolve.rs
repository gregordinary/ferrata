//! Resolution over arbitrary records: resolving never panics, and every
//! answer holds the properties `ferrata_scenario::properties` checks: the
//! same answer each time, from one resolver asked every request in turn as
//! from one built for each alone, every requirement met inside the
//! resolution, no `!` entry violated, one package to each name held,
//! nothing installed that nothing asks for, each package after what it
//! depends on but at a cycle, any set ordered with each record placed once,
//! over an installed set, changes that take the root to the resolution,
//! each package's source, and an explanation of every answer whose chains
//! end at the request or the world.
//!
//! The bytes are read as an index, up to its first error. Each record's name
//! is requested alone, and the first few names are requested together:
//! into an empty root, and into one where the first few records are
//! installed and the first is the world.

#![no_main]

use ferrata::Dialect;
use ferrata::index::{self, Located};
use ferrata::record::Record;
use ferrata::resolve::Request;
use ferrata_scenario::properties;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let records: Vec<Record> = index::read(Dialect::V3_0, input)
        .map_while(Result::ok)
        .map(Located::into_record)
        .collect();
    let together: Request = records
        .iter()
        .take(4)
        .filter_map(|record| Dialect::V3_0.dependency(record.name()).ok())
        .collect();
    let repositories = [records];
    let (installed, world) = properties::root(&repositories);
    let requests = properties::requests(&repositories, &[together]);
    for (installed, world) in [(&[][..], &[][..]), (&installed[..], &world[..])] {
        if let Err(violation) = properties::check_reuse(&repositories, installed, world, &requests)
        {
            panic!("{violation}");
        }
        for request in &requests {
            if let Err(violation) = properties::check(&repositories, installed, world, request) {
                panic!("{violation}");
            }
        }
    }
});
