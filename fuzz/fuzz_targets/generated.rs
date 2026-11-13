//! Resolution over generated repositories: the input steers the generator
//! that makes the property checks' cases (`ferrata_scenario::generate`), so
//! the fuzzer explores small repositories of well-formed records, where the
//! `resolve` target mostly meets text that does not read. Each case's
//! request, and each name alone, into an empty root and into the root
//! `properties::root` makes, must hold every property
//! `ferrata_scenario::properties` checks, and one resolver must answer them
//! all in turn as a resolver built for each alone does.

#![no_main]

use ferrata_scenario::random::Bytes;
use ferrata_scenario::{generate, properties, resolve};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let scenario = generate::scenario(&mut Bytes::new(input));
    let repositories = resolve::repositories(&scenario);
    let extra: Vec<_> = resolve::request(&scenario.request).into_iter().collect();
    let (installed, world) = properties::root(&repositories);
    let requests = properties::requests(&repositories, &extra);
    for (installed, world) in [(&[][..], &[][..]), (&installed[..], &world[..])] {
        if let Err(violation) = properties::check_reuse(&repositories, installed, world, &requests)
        {
            panic!("{violation}\n{scenario}");
        }
        for request in &requests {
            if let Err(violation) = properties::check(&repositories, installed, world, request) {
                panic!("{violation}\n{scenario}");
            }
        }
    }
});
