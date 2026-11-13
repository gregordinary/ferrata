//! The differential fuzzer: small repositories made at random, each
//! resolved by apk.static and by ferrata, and each disagreement shrunk to a
//! scenario.
//!
//! The cases are those [`generate`] makes from consecutive seeds, so a
//! disagreement is found again from its seed. A case ferrata gives no answer
//! to is neither an agreement nor a disagreement, and is reported apart,
//! with why. Two answers agree where both
//! refuse, or both install the same packages, at the same versions, in the
//! same order; ferrata's messages are its own and are not compared. A
//! disagreement is reduced by the [`reduce`](crate::reduce) module's
//! reducer, with ferrata as the second resolver, comparing the order where
//! the two install the same packages and the packages otherwise, so that the
//! reduction keeps the kind of disagreement it started from.

use std::io;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::apk::Apk;
use crate::format::{Answer, Outcome, Scenario};
use crate::generate;
use crate::random::Rng;
use crate::reduce::{Compare, Other, Reducer};
use crate::resolve;

/// A case on which apk and ferrata disagree.
#[derive(Debug, Clone)]
pub struct Finding {
    /// The seed that made it.
    pub seed: u64,
    /// The case, with apk's answer.
    pub case: Scenario,
    /// ferrata's answer.
    pub ferrata: Answer,
}

impl Finding {
    /// What the reducer compares to keep this disagreement: the order,
    /// where both install the same packages, and the packages otherwise.
    pub fn compare(&self) -> Compare {
        match &self.case.answer {
            Some(apk) if apk.outcome != Outcome::Refused && apk.same_steps(&self.ferrata) => {
                Compare::Sequence
            }
            _ => Compare::Set,
        }
    }

    /// The finding reduced: the smallest case the reducer finds on which
    /// apk and `other`, ferrata run as a command, still disagree in the same
    /// way, with apk's answer.
    ///
    /// # Errors
    ///
    /// Fails where apk or `other` cannot be run.
    pub fn reduce(&self, apk: &Apk, other: &Other) -> io::Result<Scenario> {
        let mut reducer = Reducer::new(apk, other, self.compare(), Vec::new(), |_| {});
        let mut reduced = reducer.reduce(&self.case)?;
        reduced.answer = Some(apk.answer(&reduced)?);
        reduced.statement = format!(
            "Reduced from the case seed {} makes, on which apk and ferrata disagree.",
            self.seed
        );
        Ok(reduced)
    }
}

/// What a search found.
#[derive(Debug, Clone, Default)]
pub struct Searched {
    /// The cases on which apk and ferrata disagree, in the order of their
    /// seeds.
    pub findings: Vec<Finding>,
    /// The cases ferrata gives no answer to, each by its seed with why, in
    /// the order of their seeds.
    pub unanswered: Vec<(u64, String)>,
}

/// Whether two answers agree: both refuse, or both install the same
/// packages in the same order.
pub fn agree(apk: &Answer, ferrata: &Answer) -> bool {
    apk.outcome == ferrata.outcome
}

/// Resolves the cases `seeds` make with apk and with ferrata, `jobs` at a
/// time, and gives those on which they disagree, and those ferrata gives no
/// answer to.
///
/// # Errors
///
/// Fails where apk cannot be run, or gives no answer.
pub fn search(apk: &Apk, seeds: std::ops::Range<u64>, jobs: usize) -> io::Result<Searched> {
    let next = AtomicU64::new(seeds.start);
    let findings = Mutex::new(Vec::new());
    let unanswered = Mutex::new(Vec::new());
    let failure: Mutex<Option<io::Error>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let seed = next.fetch_add(1, Ordering::Relaxed);
                    if seed >= seeds.end || failure.lock().map_or(true, |f| f.is_some()) {
                        break;
                    }
                    let case = generate::scenario(&mut Rng::new(seed));
                    let theirs = match apk.answer(&case) {
                        Ok(answer) => answer,
                        Err(error) => {
                            if let Ok(mut failure) = failure.lock() {
                                failure.get_or_insert(error);
                            }
                            break;
                        }
                    };
                    let ours = match resolve::answer(&case) {
                        Ok(ours) => ours,
                        Err(error) => {
                            if let Ok(mut unanswered) = unanswered.lock() {
                                unanswered.push((seed, error));
                            }
                            continue;
                        }
                    };
                    if !agree(&theirs, &ours)
                        && let Ok(mut findings) = findings.lock()
                    {
                        let mut case = case;
                        case.statement = format!("Made at random, from seed {seed}.");
                        case.answer = Some(theirs);
                        findings.push(Finding {
                            seed,
                            case,
                            ferrata: ours,
                        });
                    }
                }
            });
        }
    });
    if let Some(error) = failure.into_inner().ok().flatten() {
        return Err(error);
    }
    let mut searched = Searched {
        findings: findings.into_inner().unwrap_or_default(),
        unanswered: unanswered.into_inner().unwrap_or_default(),
    };
    searched.findings.sort_by_key(|finding| finding.seed);
    searched.unanswered.sort();
    Ok(searched)
}
