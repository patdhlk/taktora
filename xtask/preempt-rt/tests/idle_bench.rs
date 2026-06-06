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
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("run.ndjson");

    let mut child = Command::new(env!("CARGO_BIN_EXE_preempt-rt-bench"))
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

    // Drain stderr incrementally and wait for the start beacon: a SIGTERM
    // delivered before the run loop installs its signal handling hits the
    // default action and kills the process without a summary (flaky on a
    // slow/loaded start with a fixed sleep).
    let stderr_pipe = child.stderr.take().expect("piped stderr");
    let lines: Vec<String> = {
        let mut collected = Vec::new();
        let mut reader = BufReader::new(stderr_pipe);
        let mut beaconed = false;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).expect("read stderr") == 0 {
                break; // EOF: child exited
            }
            if !beaconed && line.contains("running") {
                beaconed = true;
                // Run loop is up; let some cycles pass, then request
                // graceful termination.
                std::thread::sleep(std::time::Duration::from_millis(200));
                // SAFETY: plain kill(2) on a child this test owns.
                #[allow(unsafe_code)]
                unsafe {
                    libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
                }
            }
            collected.push(line);
        }
        collected
    };
    assert!(child.wait().expect("wait bench").success());

    let stderr = lines.concat();
    assert!(
        stderr.contains("WARNING") && stderr.contains("requested"),
        "truncated run must warn that fewer records than requested cycles \
         were written; stderr was:\n{stderr}"
    );
}
