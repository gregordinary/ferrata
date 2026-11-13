//! The properties every resolution holds, over repositories made at random:
//! each generated case's request, and each name alone, resolved into an
//! empty root and into the root `properties::root` makes, and checked as
//! [`properties`] sets out, and all of them by one resolver in turn. A
//! failure names the seed that made the case, and `scenario generate SEED`
//! writes it out as a scenario file.

use ferrata_scenario::generate;
use ferrata_scenario::properties;
use ferrata_scenario::random::Rng;
use ferrata_scenario::resolve;

/// How many cases every run checks.
const CASES: u64 = 1_000;

#[test]
fn every_resolution_holds_its_properties() {
    let mut failures = Vec::new();
    let mut resolved = 0;
    for seed in 0..CASES {
        let scenario = generate::scenario(&mut Rng::new(seed));
        let repositories = resolve::repositories(&scenario);
        let extra: Vec<_> = resolve::request(&scenario.request).into_iter().collect();
        let (installed, world) = properties::root(&repositories);
        let requests = properties::requests(&repositories, &extra);
        for (root, installed, world) in [
            ("an empty root", &[][..], &[][..]),
            ("a root", &installed[..], &world[..]),
        ] {
            if let Err(violation) =
                properties::check_reuse(&repositories, installed, world, &requests)
            {
                failures.push(format!("seed {seed}, into {root}: {violation}"));
            }
        }
        for request in requests {
            for (root, installed, world) in [
                ("an empty root", &[][..], &[][..]),
                ("a root", &installed[..], &world[..]),
            ] {
                resolved += 1;
                if let Err(violation) = properties::check(&repositories, installed, world, &request)
                {
                    let entries: Vec<String> = request.entries().map(ToString::to_string).collect();
                    failures.push(format!(
                        "seed {seed}, request {} into {root}: {violation}",
                        entries.join(" ")
                    ));
                }
            }
        }
    }
    eprintln!("{resolved} requests over {CASES} generated cases checked");
    assert!(
        failures.is_empty(),
        "{} violations:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn generated_cases_are_well_formed_and_varied() {
    let mut with_two_repositories = 0;
    let mut with_conditions = 0;
    let mut with_conflicts = 0;
    let mut refused = 0;
    for seed in 0..1_000 {
        let scenario = generate::scenario(&mut Rng::new(seed));
        let text = scenario.to_string();
        assert_eq!(
            ferrata_scenario::format::Scenario::parse(&text).as_ref(),
            Ok(&scenario),
            "seed {seed} does not write back as itself"
        );
        with_two_repositories += usize::from(scenario.repositories.len() == 2);
        with_conditions += usize::from(text.contains("\ni:"));
        with_conflicts += usize::from(text.contains('!'));
        let answer = resolve::answer(&scenario).unwrap();
        refused += usize::from(answer.outcome == ferrata_scenario::format::Outcome::Refused);
    }
    // Each kind of input is common enough to be exercised, and a good share
    // of the requests is answered rather than refused. Refusals are common:
    // conflicts, and names provided only at no version and without a
    // priority, refuse many a small repository.
    assert!(with_two_repositories > 150, "{with_two_repositories}");
    assert!(with_conditions > 400, "{with_conditions}");
    assert!(with_conflicts > 300, "{with_conflicts}");
    assert!(refused < 600, "{refused}");
}
