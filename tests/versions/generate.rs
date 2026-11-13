//! Version strings made at random, for comparing ferrata with apk.
//!
//! Each part is drawn from a small alphabet, so that equal parts, leading
//! zeros, and parts of different kinds meeting at one position are common
//! rather than rare. A share of the strings are then damaged by one edit, to
//! find where the two disagree on what a version is.

/// A small deterministic generator (SplitMix64), so a run can be repeated
/// from its seed.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number below `bound`.
    pub(crate) fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).unwrap()).unwrap()
    }

    /// True one time in `odds`.
    pub(crate) fn one_in(&mut self, odds: usize) -> bool {
        self.below(odds) == 0
    }

    pub(crate) fn pick<'a>(&mut self, choices: &[&'a str]) -> &'a str {
        choices[self.below(choices.len())]
    }

    pub(crate) fn number(&mut self) -> &'static str {
        if self.one_in(16) {
            self.pick(LARGE)
        } else {
            self.pick(NUMBERS)
        }
    }
}

const NUMBERS: &[&str] = &["0", "1", "2", "9", "10", "01", "00", "010", "001"];
/// Numbers at and past the largest a version may hold, drawn rarely so that
/// they do not crowd out every other way a string can fail.
const LARGE: &[&str] = &[
    "18446744073709551615",
    "018446744073709551615",
    "18446744073709551616",
];
const SUFFIXES: &[&str] = &["alpha", "beta", "pre", "rc", "cvs", "svn", "git", "hg", "p"];
const COMMITS: &[&str] = &["0", "f", "a", "00", "0f", "f0", "10", "ff"];
/// What an edit inserts: the grammar's own characters, and a few it has no
/// place for.
const INSERTS: &[&str] = &[
    ".", "_", "-", "~", "r", "a", "z", "A", "0", "1", "p", "@", "+", ":",
];

/// One version, valid unless an edit made it otherwise.
pub(crate) fn string(rng: &mut Rng) -> String {
    let mut text = String::from(rng.number());
    for _ in 0..rng.below(4) {
        text.push('.');
        text.push_str(rng.number());
    }
    if rng.one_in(4) {
        text.push_str(rng.pick(&["a", "b", "z"]));
    }
    for _ in 0..rng.below(3) {
        text.push('_');
        text.push_str(rng.pick(SUFFIXES));
        if rng.one_in(2) {
            text.push_str(rng.number());
        }
    }
    if rng.one_in(5) {
        text.push('~');
        text.push_str(rng.pick(COMMITS));
    }
    if rng.one_in(2) {
        text.push_str("-r");
        text.push_str(rng.number());
    }
    if rng.one_in(4) {
        edit(rng, &mut text);
    }
    text
}

/// Inserts, removes, or swaps one character.
fn edit(rng: &mut Rng, text: &mut String) {
    let at = rng.below(text.len() + 1);
    match rng.below(3) {
        0 => text.insert_str(at, rng.pick(INSERTS)),
        1 if at < text.len() => {
            text.remove(at);
        }
        _ if at + 1 < text.len() => {
            let (a, b) = (text.as_bytes()[at], text.as_bytes()[at + 1]);
            text.replace_range(at..at + 2, &String::from_utf8(vec![b, a]).unwrap());
        }
        _ => text.push_str(rng.pick(INSERTS)),
    }
}

/// `count` strings from `seed`, none empty.
pub(crate) fn strings(seed: u64, count: usize) -> Vec<String> {
    let mut rng = Rng::new(seed);
    (0..count)
        .map(|_| string(&mut rng))
        .filter(|text| !text.is_empty())
        .collect()
}
