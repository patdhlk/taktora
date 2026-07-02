//! Capture the build's source identity at compile time and expose it to the
//! crate as `TAKTORA_BUILD_*` env constants (`BB_0123`, `REQ_0980`, `ADR_0128`).
//!
//! Zero dependencies: it shells `git`, `rustc`, and `date` through
//! [`std::process`]. Any failing command degrades that field to `"unknown"`
//! (dirty to clean), so a build with no `.git` — e.g. from a source tarball —
//! still compiles rather than failing the build.

use std::process::Command;

/// Run `cmd args…`, returning trimmed stdout, or `None` on any failure or empty
/// output.
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn emit(key: &str, value: &str) {
    println!("cargo:rustc-env={key}={value}");
}

fn main() {
    let git = |args: &[&str]| run("git", args);
    let unknown = || "unknown".to_owned();

    let sha = git(&["rev-parse", "HEAD"]).unwrap_or_else(unknown);
    let short = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(unknown);
    let describe = git(&["describe", "--tags", "--always"]).unwrap_or_else(unknown);
    // `git status --porcelain` prints one line per change; `run` returns `None`
    // on empty output, so `Some(_)` means the worktree is dirty. A git failure
    // (no repo) also yields `None` → reported clean, matching the DTO default.
    let dirty = if git(&["status", "--porcelain"]).is_some() {
        "1"
    } else {
        "0"
    };
    let timestamp = run("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_else(unknown);
    let rustc_bin = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let rustc = run(&rustc_bin, &["--version"]).unwrap_or_else(unknown);

    emit("TAKTORA_BUILD_GIT_SHA", &sha);
    emit("TAKTORA_BUILD_GIT_SHORT", &short);
    emit("TAKTORA_BUILD_GIT_DESCRIBE", &describe);
    emit("TAKTORA_BUILD_GIT_DIRTY", dirty);
    emit("TAKTORA_BUILD_TIMESTAMP", &timestamp);
    emit("TAKTORA_BUILD_RUSTC", &rustc);

    // Re-run this script when the commit moves, so a rebuild after a new commit
    // re-captures the hash instead of keeping a stale one. Emitting any
    // rerun-if-changed disables the default "rerun on any crate-file change";
    // when there is no `.git`, none are emitted and that default applies.
    for path in ["HEAD", "packed-refs"] {
        if let Some(resolved) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={resolved}");
        }
    }
    if let Some(head_ref) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        if let Some(resolved) = git(&["rev-parse", "--git-path", &head_ref]) {
            println!("cargo:rerun-if-changed={resolved}");
        }
    }
}
