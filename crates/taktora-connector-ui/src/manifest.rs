//! [`ManifestBuilder`]: accumulates ViewModel and command schemas into a
//! [`Manifest`] (`REQ_0872`, `REQ_0873`, `REQ_0874`).
//!
//! The manifest is the **sole** place service names are defined (`REQ_0873`): a
//! UI never constructs service names by convention. Every service this connector
//! creates is prefixed by an **instance namespace** (default the process name,
//! never wall-clock) so multiple taktora applications coexist on one host.
//!
//! The builder fills in each schema's service name from the instance namespace,
//! then computes the structural [`contract_hash`] over the assembled manifest.
//! The manifest itself is published — by the pump, as an exempt entry — on a
//! well-known instance-namespaced service with history depth 1, so a
//! late-joining UI receives it immediately (`REQ_0872`).

use taktora_connector_ui_contract::{CommandSchema, Manifest, ViewModelSchema, contract_hash};

use crate::pump::{EncodeFn, PumpEntry, VmPublisher};

/// The well-known suffix of the per-instance manifest service.
const MANIFEST_SUFFIX: &str = "manifest";

/// The service name carrying the manifest for `instance`.
#[must_use]
pub fn manifest_service_name(instance: &str) -> String {
    format!("{instance}.{MANIFEST_SUFFIX}")
}

/// The service name for a ViewModel `name` under `instance`.
#[must_use]
pub fn view_model_service_name(instance: &str, name: &str) -> String {
    format!("{instance}.vm.{name}")
}

/// The request service name for a command `name` under `instance`.
#[must_use]
pub fn command_request_service_name(instance: &str, name: &str) -> String {
    format!("{instance}.cmd.{name}.req")
}

/// The reply (ack) service name for a command `name` under `instance`.
#[must_use]
pub fn command_reply_service_name(instance: &str, name: &str) -> String {
    format!("{instance}.cmd.{name}.rep")
}

/// The `CanExecute` gate service name for a command `name` under `instance`.
#[must_use]
pub fn can_execute_service_name(instance: &str, name: &str) -> String {
    format!("{instance}.cmd.{name}.can")
}

/// The default instance namespace: the current executable's file stem, falling
/// back to `pid_<id>`.
///
/// Never derived from wall-clock (per the connector's no-ambient-time rule). The
/// stem is sanitised to `[A-Za-z0-9_]` so it is a valid service-name component.
#[must_use]
pub fn default_instance() -> String {
    let stem = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .map(|s| sanitize(&s))
        .filter(|s| !s.is_empty());
    stem.unwrap_or_else(|| format!("pid_{}", std::process::id()))
}

/// Replace every character outside `[A-Za-z0-9_]` with `_`.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Accumulates schemas and produces a fully-namespaced [`Manifest`].
pub struct ManifestBuilder {
    instance: String,
    epoch: u64,
    view_models: Vec<ViewModelSchema>,
    commands: Vec<CommandSchema>,
}

impl ManifestBuilder {
    /// Start a builder for `instance` with process `epoch`.
    #[must_use]
    pub fn new(instance: impl Into<String>, epoch: u64) -> Self {
        Self {
            instance: instance.into(),
            epoch,
            view_models: Vec::new(),
            commands: Vec::new(),
        }
    }

    /// The instance namespace.
    #[must_use]
    pub fn instance(&self) -> &str {
        &self.instance
    }

    /// Add a ViewModel schema; its `service` field is filled with the
    /// instance-namespaced ViewModel service name (overwriting any value the
    /// caller's `schema()` left blank).
    #[must_use]
    pub fn with_view_model(mut self, mut schema: ViewModelSchema) -> Self {
        schema.service = view_model_service_name(&self.instance, &schema.name);
        self.view_models.push(schema);
        self
    }

    /// Add a command schema; its request / reply (and `can_execute`, if present)
    /// service names are filled from the instance namespace.
    #[must_use]
    pub fn with_command(mut self, mut schema: CommandSchema) -> Self {
        schema.request_service = command_request_service_name(&self.instance, &schema.name);
        schema.reply_service = command_reply_service_name(&self.instance, &schema.name);
        if schema.can_execute_service.is_some() {
            schema.can_execute_service =
                Some(can_execute_service_name(&self.instance, &schema.name));
        }
        self.commands.push(schema);
        self
    }

    /// Finish: compute the structural contract hash and return the manifest.
    #[must_use]
    pub fn build(self) -> Manifest {
        let mut manifest = Manifest {
            instance: self.instance,
            epoch: self.epoch,
            contract_hash: String::new(),
            view_models: self.view_models,
            commands: self.commands,
        };
        manifest.contract_hash = contract_hash(&manifest);
        manifest
    }
}

/// Build the exempt pump entry that publishes the assembled [`Manifest`] JSON
/// (`REQ_0872`).
///
/// Like [`system_entry`](crate::system::system_entry) the entry is **exempt**
/// from the zero-subscriber skip and re-serializes the (immutable) manifest
/// every tick, always reporting a change so it publishes each tick. Combined
/// with the manifest service's history depth 1, a UI that joins late receives
/// the current manifest within one pump tick — no resync handshake.
#[must_use]
pub fn manifest_entry<P>(manifest: Manifest, publisher: P) -> PumpEntry
where
    P: VmPublisher + 'static,
{
    let encode: EncodeFn = Box::new(move |out: &mut Vec<u8>| {
        out.clear();
        // Serializing a plain-data manifest is infallible; on the off chance it
        // errs we report "no value this tick" rather than panicking on the pump
        // thread (mirroring `system_entry`).
        serde_json::to_writer(&mut *out, &manifest).ok()?;
        Some(true)
    });
    PumpEntry::new(MANIFEST_SUFFIX, true, encode, Box::new(publisher))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pump::{MockPublisher, Pump};
    use taktora_connector_ui_contract::{FieldSchema, FieldType, Kind};

    fn vm(name: &str) -> ViewModelSchema {
        ViewModelSchema {
            name: name.into(),
            service: String::new(),
            fields: vec![FieldSchema {
                name: "position".into(),
                ty: FieldType::F64,
            }],
        }
    }

    fn cmd(name: &str, can: bool) -> CommandSchema {
        CommandSchema {
            name: name.into(),
            request_service: String::new(),
            reply_service: String::new(),
            params: vec![],
            kind: Kind::Command,
            idempotent: true,
            can_execute_service: if can { Some(String::new()) } else { None },
        }
    }

    #[test]
    fn builder_namespaces_all_service_names() {
        let m = ManifestBuilder::new("app", 1)
            .with_view_model(vm("Stepper"))
            .with_command(cmd("enable", true))
            .build();

        assert_eq!(m.view_models[0].service, "app.vm.Stepper");
        assert_eq!(m.commands[0].request_service, "app.cmd.enable.req");
        assert_eq!(m.commands[0].reply_service, "app.cmd.enable.rep");
        assert_eq!(
            m.commands[0].can_execute_service.as_deref(),
            Some("app.cmd.enable.can")
        );
    }

    #[test]
    fn absent_can_execute_stays_absent() {
        let m = ManifestBuilder::new("app", 1)
            .with_command(cmd("jog", false))
            .build();
        assert!(m.commands[0].can_execute_service.is_none());
    }

    #[test]
    fn builder_carries_instance_epoch_and_schema_names() {
        let m = ManifestBuilder::new("app", 42)
            .with_view_model(vm("Stepper"))
            .build();
        assert_eq!(m.instance, "app");
        assert_eq!(m.epoch, 42);
        assert_eq!(m.view_models[0].name, "Stepper");
    }

    #[test]
    fn build_computes_the_structural_hash() {
        let m = ManifestBuilder::new("app", 1)
            .with_view_model(vm("Stepper"))
            .build();
        // 64 lowercase hex chars, and equal to a fresh recomputation.
        assert_eq!(m.contract_hash.len(), 64);
        assert_eq!(m.contract_hash, contract_hash(&m));
    }

    #[test]
    fn hash_is_independent_of_instance_namespace() {
        let a = ManifestBuilder::new("app_a", 1)
            .with_view_model(vm("Stepper"))
            .build();
        let b = ManifestBuilder::new("app_b", 99)
            .with_view_model(vm("Stepper"))
            .build();
        // Same structure, different instance + epoch + service names -> same hash.
        assert_eq!(a.contract_hash, b.contract_hash);
        assert_ne!(a.view_models[0].service, b.view_models[0].service);
    }

    #[test]
    fn manifest_service_name_is_instance_prefixed() {
        assert_eq!(manifest_service_name("app"), "app.manifest");
    }

    #[test]
    fn default_instance_is_non_empty_and_sanitised() {
        let inst = default_instance();
        assert!(!inst.is_empty());
        assert!(inst.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    #[test]
    fn manifest_entry_is_exempt_and_publishes_parseable_json_every_tick() {
        let manifest = ManifestBuilder::new("app", 3)
            .with_view_model(vm("Stepper"))
            .build();
        let mock = MockPublisher::new(); // zero subscribers on purpose
        let mut pump = Pump::new();
        pump.add_entry(manifest_entry(manifest.clone(), mock.clone()));

        // Exempt: publishes even with no subscribers, every tick.
        let s1 = pump.tick();
        let s2 = pump.tick();
        assert_eq!(s1.published, 1);
        assert_eq!(s1.skipped_zero_sub, 0);
        assert_eq!(s2.published, 1);
        assert_eq!(mock.publish_count(), 2);

        // The payload round-trips back to the source manifest.
        let parsed: Manifest = serde_json::from_slice(&mock.last_published().unwrap()).unwrap();
        assert_eq!(parsed, manifest);
        assert!(!parsed.contract_hash.is_empty());
    }
}
