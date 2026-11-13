//! Records the commit the tooling is built from, so that a sweep's results
//! name the ferrata that gave them: `FERRATA_COMMIT` is the commit, with
//! `+modified` where the library or the tooling differs from it, or
//! `unknown` outside a git checkout.

use std::process::Command;

/// What `git` prints for `arguments`, run in the tooling's directory.
fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    // What the stamp describes, and so what a change to reruns this for.
    let sources = [
        "../src",
        "../Cargo.toml",
        "../Cargo.lock",
        "src",
        "Cargo.toml",
        "build.rs",
    ];
    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    let stamp = match git(&["rev-parse", "HEAD"]) {
        Some(commit) => {
            let mut status = vec!["status", "--porcelain", "--untracked-files=no", "--"];
            status.extend(sources);
            let modified = git(&status).is_none_or(|changes| !changes.is_empty());
            // A commit moves the branch, or detaches HEAD.
            let mut watched = vec![git(&[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "HEAD",
            ])];
            if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
                watched.push(git(&[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    &branch,
                ]));
            }
            for path in watched.into_iter().flatten() {
                println!("cargo:rerun-if-changed={path}");
            }
            if modified {
                format!("{commit}+modified")
            } else {
                commit
            }
        }
        None => "unknown".to_owned(),
    };
    println!("cargo:rustc-env=FERRATA_COMMIT={stamp}");
}
