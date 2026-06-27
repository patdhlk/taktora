//! egui/eframe reference View for the MVVM UI connector (`FEAT_0092`, Task 6.2).
//!
//! A minimal operator panel that binds the `ui-demo` producer purely over the
//! published, language-neutral JSON contract via
//! [`taktora_connector_ui_client::Client`] — it depends on neither the executor
//! nor the server crate. It shows the live `position`, `state`, and the `System`
//! heartbeat counter, grays out the **Jog** button whenever
//! `can_execute("jog_relative")` is false, and invokes `enable` / `jog_relative`
//! on the connector.
//!
//! This file is deliberately thin UI glue: the interesting logic (the
//! simulation, the contract) is tested elsewhere. It must compile and be
//! sensible; it is not unit tested.
//!
//! Run the producer first (`cd ../ui-demo && cargo run`), then `cargo run` here.

use std::time::Duration;

use eframe::egui;
use serde_json::{Value, json};
use taktora_connector_ui_client::{Client, ClientError, CommandOutcome};

/// The instance namespace the `ui-demo` producer publishes under.
const INSTANCE: &str = "ui-demo";
/// The relative jog applied each time the operator presses **Jog**.
const JOG_DELTA: f64 = 5.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([360.0, 280.0]),
        ..Default::default()
    };
    eframe::run_native(
        "taktora ui-demo View",
        options,
        Box::new(|_cc| Ok(Box::<DemoView>::default())),
    )
}

/// The View's whole state: an optional bound client plus the last action result.
#[derive(Default)]
struct DemoView {
    client: Option<Client>,
    subscribed: bool,
    last_action: String,
}

impl DemoView {
    /// Attempt to bind the running producer in read-write mode.
    ///
    /// A real generated client embeds the contract hash it was built against.
    /// This demo View doesn't have one ahead of time, so it probes once (any
    /// hash still reads the manifest), learns the live `contract_hash`, then
    /// reconnects with it to obtain a read-write binding.
    fn try_connect() -> Result<Client, ClientError> {
        let probe = Client::connect(INSTANCE, "probe")?;
        let hash = probe.manifest().contract_hash.clone();
        Client::connect(INSTANCE, &hash)
    }

    /// Subscribe to everything the panel reads, once, right after connecting.
    fn ensure_subscribed(client: &mut Client) {
        let _ = client.subscribe("Stepper");
        let _ = client.subscribe("System");
        let _ = client.subscribe_can_execute("jog_relative");
    }

    /// Drain fresh samples so the cached views/gates reflect the latest values.
    fn pump(client: &mut Client) {
        let _ = client.poll_view_model("Stepper");
        let _ = client.poll_view_model("System");
        let _ = client.poll_can_execute("jog_relative");
    }
}

/// Read a named field from a cached ViewModel object as an owned JSON value.
fn field(client: &Client, vm: &str, name: &str) -> Option<Value> {
    client
        .view_model_fields(vm)
        .and_then(|m| m.get(name).cloned())
}

/// Render a JSON scalar without surrounding quotes (so a state string prints as
/// `Running`, not `"Running"`).
fn scalar(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => "—".to_owned(),
    }
}

impl eframe::App for DemoView {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Lazily (re)connect. The producer may not be up yet on first frame.
        if self.client.is_none() {
            match Self::try_connect() {
                Ok(c) => {
                    self.client = Some(c);
                    self.subscribed = false;
                }
                Err(_) => { /* keep trying each frame */ }
            }
        }
        if let Some(client) = self.client.as_mut() {
            if !self.subscribed {
                Self::ensure_subscribed(client);
                self.subscribed = true;
            }
            Self::pump(client);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("taktora ui-demo");

            let Some(client) = self.client.as_mut() else {
                ui.add_space(8.0);
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!("Connecting to instance '{INSTANCE}'… is the producer running?"),
                );
                ctx.request_repaint_after(Duration::from_millis(200));
                return;
            };

            ui.add_space(4.0);
            ui.label(format!(
                "mode: {:?}   epoch: {}",
                client.mode(),
                client.epoch()
            ));
            ui.separator();

            // Live ViewModel fields.
            let position = scalar(field(client, "Stepper", "position").as_ref());
            let state = scalar(field(client, "Stepper", "state").as_ref());
            let heartbeat = scalar(field(client, "System", "counter").as_ref());
            let can_jog = client.can_execute("jog_relative").unwrap_or(false);

            egui::Grid::new("vm")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("state:");
                    ui.strong(state);
                    ui.end_row();
                    ui.label("position:");
                    ui.strong(position);
                    ui.end_row();
                    ui.label("heartbeat:");
                    ui.strong(heartbeat);
                    ui.end_row();
                    ui.label("can_jog:");
                    ui.strong(can_jog.to_string());
                    ui.end_row();
                });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Enable").clicked() {
                    self.last_action = describe(client.invoke("enable", &json!({ "force": true })));
                }
                // Gray out Jog unless the connector currently permits it.
                let jog = ui.add_enabled(can_jog, egui::Button::new("Jog +5"));
                if jog.clicked() {
                    self.last_action =
                        describe(client.invoke("jog_relative", &json!({ "delta": JOG_DELTA })));
                }
            });

            if !self.last_action.is_empty() {
                ui.add_space(4.0);
                ui.label(format!("last action: {}", self.last_action));
            }
        });

        // Keep polling at ~30 Hz so the panel tracks the live producer.
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

/// Turn a command result into a one-line status for the panel.
fn describe(result: Result<CommandOutcome, ClientError>) -> String {
    match result {
        Ok(CommandOutcome::Accepted) => "accepted".to_owned(),
        Ok(CommandOutcome::Rejected { code, message }) => {
            format!("rejected: {code:?} ({message})")
        }
        Ok(CommandOutcome::OutcomeUnknown) => "outcome unknown (epoch changed)".to_owned(),
        Err(e) => format!("error: {e:?}"),
    }
}
