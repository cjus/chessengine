//! Records the commit cb2026 is built from, so a client can tell two builds of one version
//! apart (`build_version` in `lib.rs`): `CB2026_COMMIT`, the commit's first `COMMIT_LENGTH`
//! hex digits.
//!
//! It asks git, which works in a clone, a worktree and the checkout `cargo install --git`
//! builds from. Where there is no git or no repository (a source archive), it records
//! nothing, and the version is the plain one.
//!
//! Pitfall: the build script reruns when the commit changes (HEAD, or the branch it points
//! to), not when a file is edited, so a build with uncommitted changes names the commit it
//! started from. Cargo would otherwise rerun it on every change to the crate.

use std::path::{Path, PathBuf};
use std::process::Command;

/// How many hex digits of the commit to record: enough to be unique in a repository this size,
/// as `git log --abbrev=12` is.
const COMMIT_LENGTH: usize = 12;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let Some(commit) = git(&["rev-parse", "HEAD"]) else {
        return;
    };
    if let (Some(git_dir), Some(common_dir)) = (
        git(&["rev-parse", "--git-dir"]),
        git(&["rev-parse", "--git-common-dir"]),
    ) {
        let (git_dir, common_dir) = (absolute(&git_dir), absolute(&common_dir));
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        println!(
            "cargo:rerun-if-changed={}",
            common_dir.join("packed-refs").display()
        );
        if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!(
                "cargo:rerun-if-changed={}",
                common_dir.join(branch).display()
            );
        }
    }
    let short = &commit[..COMMIT_LENGTH.min(commit.len())];
    println!("cargo:rustc-env=CB2026_COMMIT={short}");
}

/// What `git args` prints, trimmed, if it succeeds.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (output.status.success() && !text.is_empty()).then(|| text.to_string())
}

/// `path` from git, which may be relative to the crate's directory, made absolute.
fn absolute(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}
