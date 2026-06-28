//! The mandatory Area/Component grouping manifest for `taktora-medkit`.
//!
//! medkit v1 does no service discovery, so the Area/Component skeleton that the
//! SOVD entity tree groups under cannot be inferred from the running system: the
//! bindings emit only flat, raw entities (`app:<task>`, `component:<subdevice>`).
//! The manifest supplies that skeleton and the entity→parent mapping the merge
//! pipeline re-parents the raw entities into (`REQ_0920`, `REQ_0921`, `ADR_0113`).
//!
//! Two surfaces over one shape:
//!
//! - **Builder core** — [`Manifest::builder`] gives a type-safe, programmatic
//!   way to declare the skeleton and mappings, for tests and in-code wiring.
//! - **TOML loader** — [`Manifest::from_toml`] deserialises the same shape from a
//!   committed `medkit.toml`, so ops edit the topology without recompiling.
//!
//! Both paths build the *same* [`Manifest`] value, pinned by an equality test, so
//! the two surfaces never drift.
//!
//! A missing or empty manifest is not an error: [`Manifest::is_empty`] lets the
//! pipeline fall back to flat grouping (the pre-manifest behaviour) instead of
//! failing (`REQ_0922`).
//!
//! This crate carries **zero** taktora dependencies — `serde` + `toml` over the
//! `taktora-medkit-model` DTOs — holding the extractable-core invariant
//! (`REQ_0916`, `ADR_0111`).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;
use taktora_medkit_model::{Entity, EntityKind, EntityMeta};

/// The id prefix a binding emits a per-task app entity under (`app:<task>`).
pub const APP_PREFIX: &str = "app:";

/// The id prefix a binding emits a per-subdevice component entity under
/// (`component:<subdevice>`).
pub const SUBDEVICE_PREFIX: &str = "component:";

/// The `source` tag stamped on the `x-medkit` of manifest-declared entities, so a
/// client can tell a declared Area/Component apart from a runtime-discovered one.
pub const MANIFEST_SOURCE: &str = "manifest";

/// A declared Area: a top-level grouping the components hang under.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct Area {
    /// Stable entity id (also the `/areas/{id}` path segment).
    pub id: String,
    /// Human-readable name.
    pub name: String,
}

/// A declared Component, parented to a declared [`Area`].
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct Component {
    /// Stable entity id (also the `/components/{id}` path segment).
    pub id: String,
    /// The id of the [`Area`] this component groups under.
    pub area: String,
    /// Human-readable name.
    pub name: String,
}

/// The Area/Component grouping skeleton plus the entity→parent mappings.
///
/// Build one programmatically with [`Manifest::builder`] or load it from TOML
/// with [`Manifest::from_toml`]; both yield an identical value. The merge pipeline
/// reads the declared entities via [`Manifest::declared_entities`] and the
/// re-parent target of a raw entity via [`Manifest::parent_of`].
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct Manifest {
    areas: Vec<Area>,
    components: Vec<Component>,
    /// task id → parent entity id (`app:<task>` re-parents to `parent`).
    tasks: BTreeMap<String, String>,
    /// subdevice address → component id (`component:<addr>` re-parents to it).
    subdevices: BTreeMap<String, String>,
}

impl Manifest {
    /// Start a [`ManifestBuilder`].
    #[must_use]
    pub fn builder() -> ManifestBuilder {
        ManifestBuilder::default()
    }

    /// Load a manifest from a TOML file.
    ///
    /// # Errors
    ///
    /// [`ManifestError::Io`] if the file cannot be read, or
    /// [`ManifestError::Parse`] if its contents are not the manifest TOML shape.
    pub fn from_toml(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let raw = std::fs::read_to_string(path)?;
        Self::from_toml_str(&raw)
    }

    /// Parse a manifest from a TOML string (the in-memory form of
    /// [`Manifest::from_toml`]).
    ///
    /// # Errors
    ///
    /// [`ManifestError::Parse`] if `toml` is not the manifest TOML shape.
    pub fn from_toml_str(toml: &str) -> Result<Self, ManifestError> {
        let file: ManifestFile = ::toml::from_str(toml)?;
        Ok(file.into())
    }

    /// Whether the manifest declares nothing. An empty manifest signals the merge
    /// pipeline to fall back to flat grouping rather than re-parenting
    /// (`REQ_0922`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.areas.is_empty()
            && self.components.is_empty()
            && self.tasks.is_empty()
            && self.subdevices.is_empty()
    }

    /// The declared areas, in declaration order.
    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    /// The declared components, in declaration order.
    #[must_use]
    pub fn components(&self) -> &[Component] {
        &self.components
    }

    /// The re-parent target for a raw provider entity id, if the manifest maps it.
    ///
    /// Recognises the binding id conventions: `app:<task>` resolves through the
    /// task mappings, `component:<subdevice>` through the subdevice mappings. Any
    /// other id (including the manifest's own declared Areas/Components) returns
    /// `None`.
    #[must_use]
    pub fn parent_of(&self, raw_id: &str) -> Option<&str> {
        if let Some(task) = raw_id.strip_prefix(APP_PREFIX) {
            return self.tasks.get(task).map(String::as_str);
        }
        if let Some(addr) = raw_id.strip_prefix(SUBDEVICE_PREFIX) {
            return self.subdevices.get(addr).map(String::as_str);
        }
        None
    }

    /// The declared skeleton as model entities: every [`Area`] (parentless) then
    /// every [`Component`] (parented to its area), each stamped
    /// `x-medkit.source = "manifest"`.
    ///
    /// The merge pipeline upserts these into the read-model before re-parenting
    /// the raw provider entities under them.
    #[must_use]
    pub fn declared_entities(&self) -> Vec<Entity> {
        let mut entities = Vec::with_capacity(self.areas.len() + self.components.len());
        for area in &self.areas {
            entities.push(declared_entity(
                EntityKind::Area,
                "areas",
                &area.id,
                &area.name,
                None,
            ));
        }
        for component in &self.components {
            entities.push(declared_entity(
                EntityKind::Component,
                "components",
                &component.id,
                &component.name,
                Some(component.area.clone()),
            ));
        }
        entities
    }
}

/// Build a manifest entity with the declared-skeleton decoration.
fn declared_entity(
    kind: EntityKind,
    collection: &str,
    id: &str,
    name: &str,
    parent_id: Option<String>,
) -> Entity {
    Entity {
        href: format!("/api/v1/{collection}/{id}"),
        id: id.to_owned(),
        name: name.to_owned(),
        kind,
        parent_id,
        description: None,
        x_medkit: Some(EntityMeta {
            source: Some(MANIFEST_SOURCE.to_owned()),
            ..EntityMeta::default()
        }),
    }
}

/// A fluent, type-safe builder for a [`Manifest`].
///
/// ```
/// use taktora_medkit_manifest::Manifest;
///
/// let manifest = Manifest::builder()
///     .area("drive", "Drive train")
///     .component("nav", "drive", "Navigation")
///     .map_task("planner", "nav")
///     .map_subdevice("0x01", "nav")
///     .build();
///
/// assert_eq!(manifest.parent_of("app:planner"), Some("nav"));
/// assert_eq!(manifest.parent_of("component:0x01"), Some("nav"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct ManifestBuilder {
    manifest: Manifest,
}

impl ManifestBuilder {
    /// Declare an [`Area`] with the given id and name.
    #[must_use]
    pub fn area(mut self, id: impl Into<String>, name: impl Into<String>) -> Self {
        self.manifest.areas.push(Area {
            id: id.into(),
            name: name.into(),
        });
        self
    }

    /// Declare a [`Component`] under the area `area`.
    #[must_use]
    pub fn component(
        mut self,
        id: impl Into<String>,
        area: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        self.manifest.components.push(Component {
            id: id.into(),
            area: area.into(),
            name: name.into(),
        });
        self
    }

    /// Map the raw `app:<task_id>` entity onto the parent entity `parent`
    /// (a declared component), hosting it there.
    #[must_use]
    pub fn map_task(mut self, task_id: impl Into<String>, parent: impl Into<String>) -> Self {
        self.manifest.tasks.insert(task_id.into(), parent.into());
        self
    }

    /// Map the raw `component:<addr>` subdevice entity onto the declared
    /// `component`, nesting it there as a subcomponent.
    #[must_use]
    pub fn map_subdevice(mut self, addr: impl Into<String>, component: impl Into<String>) -> Self {
        self.manifest
            .subdevices
            .insert(addr.into(), component.into());
        self
    }

    /// Finish building.
    #[must_use]
    pub fn build(self) -> Manifest {
        self.manifest
    }
}

/// The on-disk TOML shape: `[[area]]`, `[[component]]`, `[[task]]`,
/// `[[subdevice]]` array-of-tables, lowered into a [`Manifest`].
#[derive(Debug, Default, Deserialize)]
struct ManifestFile {
    #[serde(default, rename = "area")]
    areas: Vec<Area>,
    #[serde(default, rename = "component")]
    components: Vec<Component>,
    #[serde(default, rename = "task")]
    tasks: Vec<TaskMapping>,
    #[serde(default, rename = "subdevice")]
    subdevices: Vec<SubdeviceMapping>,
}

#[derive(Debug, Deserialize)]
struct TaskMapping {
    id: String,
    parent: String,
}

#[derive(Debug, Deserialize)]
struct SubdeviceMapping {
    addr: String,
    component: String,
}

impl From<ManifestFile> for Manifest {
    fn from(file: ManifestFile) -> Self {
        Self {
            areas: file.areas,
            components: file.components,
            tasks: file.tasks.into_iter().map(|t| (t.id, t.parent)).collect(),
            subdevices: file
                .subdevices
                .into_iter()
                .map(|s| (s.addr, s.component))
                .collect(),
        }
    }
}

/// A failure loading a [`Manifest`] from TOML.
#[derive(Debug)]
pub enum ManifestError {
    /// The manifest file could not be read.
    Io(std::io::Error),
    /// The manifest contents were not the expected TOML shape.
    Parse(::toml::de::Error),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "reading manifest: {e}"),
            Self::Parse(e) => write!(f, "parsing manifest TOML: {e}"),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ManifestError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<::toml::de::Error> for ManifestError {
    fn from(e: ::toml::de::Error) -> Self {
        Self::Parse(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same skeleton expressed as TOML, byte-aligned to the committed
    /// `medkit.toml` example shape.
    const TOML: &str = r#"
        [[area]]
        id = "drive"
        name = "Drive train"

        [[area]]
        id = "sensing"
        name = "Sensing"

        [[component]]
        id = "nav"
        area = "drive"
        name = "Navigation"

        [[component]]
        id = "lidar"
        area = "sensing"
        name = "Lidar"

        [[task]]
        id = "planner"
        parent = "nav"

        [[task]]
        id = "controller"
        parent = "nav"

        [[subdevice]]
        addr = "0x01"
        component = "nav"
    "#;

    fn built() -> Manifest {
        Manifest::builder()
            .area("drive", "Drive train")
            .area("sensing", "Sensing")
            .component("nav", "drive", "Navigation")
            .component("lidar", "sensing", "Lidar")
            .map_task("planner", "nav")
            .map_task("controller", "nav")
            .map_subdevice("0x01", "nav")
            .build()
    }

    /// `TEST_0909` — the builder and the TOML loader produce identical manifests,
    /// so the two surfaces over one shape never drift (`REQ_0920`).
    #[test]
    fn builder_and_toml_agree() {
        let from_toml = Manifest::from_toml_str(TOML).expect("parse manifest TOML");
        assert_eq!(built(), from_toml);
    }

    /// `TEST_0909` — the committed `medkit.toml` example parses into a non-empty
    /// manifest (`REQ_0920`).
    #[test]
    fn committed_example_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/medkit.toml");
        let manifest = Manifest::from_toml(path).expect("load committed medkit.toml");
        assert!(!manifest.is_empty());
        assert!(!manifest.areas().is_empty());
        assert!(!manifest.components().is_empty());
    }

    /// `TEST_0909` — `parent_of` resolves the binding id conventions and rejects
    /// unmapped / unprefixed ids.
    #[test]
    fn parent_of_resolves_conventions() {
        let manifest = built();
        assert_eq!(manifest.parent_of("app:planner"), Some("nav"));
        assert_eq!(manifest.parent_of("component:0x01"), Some("nav"));
        assert_eq!(manifest.parent_of("app:unmapped"), None);
        assert_eq!(manifest.parent_of("nav"), None);
    }

    /// `TEST_0909` — `declared_entities` yields parentless areas then
    /// area-parented components, each tagged as manifest-sourced (`REQ_0921`).
    #[test]
    fn declared_entities_carry_hierarchy() {
        let entities = built().declared_entities();
        let nav = entities
            .iter()
            .find(|e| e.id == "nav")
            .expect("declared component nav");
        assert_eq!(nav.kind, EntityKind::Component);
        assert_eq!(nav.parent_id.as_deref(), Some("drive"));
        assert_eq!(
            nav.x_medkit.as_ref().and_then(|m| m.source.as_deref()),
            Some(MANIFEST_SOURCE)
        );

        let drive = entities
            .iter()
            .find(|e| e.id == "drive")
            .expect("declared area drive");
        assert_eq!(drive.kind, EntityKind::Area);
        assert!(drive.parent_id.is_none());
    }

    /// `TEST_0911` — the default manifest is empty, so the pipeline falls back to
    /// flat grouping (`REQ_0922`).
    #[test]
    fn default_is_empty() {
        assert!(Manifest::default().is_empty());
        assert!(Manifest::from_toml_str("").expect("empty TOML").is_empty());
        assert!(!built().is_empty());
    }
}
