//! End-to-end: run the bench binary for a few cycles and assert it writes one
//! NDJSON line per cycle.
use std::process::Command;

#[test]
fn idle_run_emits_one_line_per_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("run.ndjson");

    let status = Command::new(env!("CARGO_BIN_EXE_preempt-rt-bench"))
        .args([
            "--cycles",
            "25",
            "--period-us",
            "200",
            "--ring-capacity",
            "1024",
            "--out",
        ])
        .arg(&out)
        .status()
        .expect("run bench");
    assert!(status.success(), "bench exited with failure");

    let text = std::fs::read_to_string(&out).expect("read ndjson");
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 25, "one NDJSON record per cycle");
    assert!(lines[0].contains("\"task_id\":0"));
    assert!(lines[0].contains("\"cycle_index\":0"));
    assert!(lines.last().unwrap().contains("\"cycle_index\":24"));
}

/// A run that terminates early (SIGTERM here; any graceful early exit) must
/// say so on stderr — a consumer of a "60k-cycle envelope" cannot otherwise
/// tell it was cut short (the summary line alone is easy to misread).
#[cfg(unix)]
#[test]
fn truncated_run_warns_on_stderr() {
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("run.ndjson");

    let child = Command::new(env!("CARGO_BIN_EXE_preempt-rt-bench"))
        .args([
            "--cycles",
            "100000",
            "--period-us",
            "1000",
            "--ring-capacity",
            "131072",
            "--out",
        ])
        .arg(&out)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bench");

    // Let it produce a few hundred records, then request termination.
    std::thread::sleep(std::time::Duration::from_millis(300));
    // SAFETY: plain kill(2) on a child this test owns.
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }

    let output = child.wait_with_output().expect("wait bench");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("WARNING") && stderr.contains("requested"),
        "truncated run must warn that fewer records than requested cycles \
         were written; stderr was:\n{stderr}"
    );
}
