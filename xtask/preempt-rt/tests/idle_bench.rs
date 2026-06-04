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
