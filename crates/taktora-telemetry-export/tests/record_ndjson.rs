//! NDJSON serialization of `PodRecord` — faithful schema (`REQ_0111` amended):
//! absent fields render as JSON `null`, faulted is a boolean.
use taktora_telemetry_export::PodRecord;

fn line_of(rec: &PodRecord) -> String {
    let mut buf = Vec::new();
    rec.write_ndjson(&mut buf).expect("write");
    String::from_utf8(buf).expect("utf8")
}

#[test]
fn healthy_cycle_renders_all_numbers() {
    let rec = PodRecord::new_healthy(
        /* cycle_index */ 7, /* task_index */ 2, /* ts_ns */ 1_000,
        /* period_ns */ 1_000_000, /* actual_period_ns */ 1_000_100,
        /* jitter_ns */ 100, /* lateness_ns */ -50, /* took_ns */ 250_000,
    );
    assert_eq!(
        line_of(&rec),
        "{\"cycle_index\":7,\"task_id\":2,\"faulted\":false,\"ts_ns\":1000,\
\"period_ns\":1000000,\"actual_period_ns\":1000100,\"jitter_ns\":100,\
\"lateness_ns\":-50,\"took_ns\":250000}\n"
    );
}

#[test]
fn faulted_cycle_renders_nulls() {
    // A faulted scan advances cycle_index but measures nothing (REQ_0107).
    let rec = PodRecord::new_faulted(
        /* cycle_index */ 8, /* task_index */ 2, /* ts_ns */ 2_000,
        /* period_ns */ 1_000_000,
    );
    assert_eq!(
        line_of(&rec),
        "{\"cycle_index\":8,\"task_id\":2,\"faulted\":true,\"ts_ns\":2000,\
\"period_ns\":1000000,\"actual_period_ns\":null,\"jitter_ns\":null,\
\"lateness_ns\":null,\"took_ns\":null}\n"
    );
}
