//! The whole-archive sweep, behind an environment variable: it fetches from
//! Alpine's mirrors and runs apk.static tens of thousands of times.
//!
//! With `FERRATA_SWEEP` naming a directory, every name of the pinned
//! release's `main` and `community` is resolved alone by apk.static and by
//! ferrata, and the results are written to `sweep.txt` in it, as
//! [`sweep`](ferrata_scenario::sweep) describes. The directory is filled by
//! `scenario fetch` first where it is empty, and reused as it is otherwise,
//! so a second run compares over the same indexes. `FERRATA_SWEEP_SHARD`
//! (`I/N`) runs one shard, into `sweep-I-of-N.txt`. The test fails where
//! either side cannot answer; how far the answers agree is the measurement,
//! printed at the end, not a verdict.

use std::path::PathBuf;

use ferrata_scenario::fetch::{self, Layout};
use ferrata_scenario::sweep::{self, Options, Requests, Shard};

#[test]
fn the_archive_resolves_as_apk_resolves_it() {
    let Some(directory) = std::env::var_os("FERRATA_SWEEP").map(PathBuf::from) else {
        eprintln!("FERRATA_SWEEP is not set; nothing swept");
        return;
    };
    if !directory.join("repositories").exists() {
        std::fs::create_dir_all(&directory).unwrap();
        fetch::fetch(
            &directory,
            ferrata_scenario::apk::ARCHITECTURE,
            &fetch::default_repositories(),
            &[],
            false,
        )
        .unwrap();
    }
    let shard = std::env::var("FERRATA_SWEEP_SHARD")
        .ok()
        .map(|text| Shard::parse(&text).unwrap_or_else(|| panic!("{text} is not I/N")));
    let options = Options {
        apk: std::env::var_os("FERRATA_APK_STATIC").map(PathBuf::from),
        requests: Requests::Each,
        shard,
        jobs: std::thread::available_parallelism().map_or(1, usize::from),
        base: None,
    };
    let layout = Layout::read(&directory).unwrap();
    let path = sweep::shard_path(&directory.join("sweep.txt"), shard);
    let mut out = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    let counts = sweep::sweep(&layout, &options, &mut out, |line| eprintln!("{line}")).unwrap();
    eprintln!("{counts}, written to {}", path.display());
}
