//! Opaque capture of unrecognised vendor-extension XML.
//!
//! Uses the low-level `quick_xml::reader::Reader` event API (quick-xml 0.40)
//! because serde-derive silently discards unknown elements, which would defeat
//! `REQ_0505` (faithful capture of vendor extensions). The serde pass in
//! [`crate::parse`] handles the known schema; this module makes a second,
//! read-only pass over the same `&str` to harvest the device-level elements the
//! schema does not recognise.

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::error::EsiError;
use crate::position::LineIndex;

/// An opaque XML element preserved verbatim (name, attributes, text, children).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawXml {
    /// Element name as quick-xml reports it (qualified, e.g. `Beckhoff:Foo`).
    pub name: String,
    /// Attributes as `(name, value)` pairs, in document order.
    pub attributes: Vec<(String, String)>,
    /// Direct text content, when present.
    pub text: Option<String>,
    /// Child elements, recursively.
    pub children: Vec<Self>,
}

/// Direct `<Device>` children that belong to the known schema and are therefore
/// NOT captured as vendor extensions. Everything else under a `<Device>` is
/// captured verbatim.
const KNOWN_DEVICE_CHILDREN: &[&str] = &[
    "Type",
    "Name",
    "GroupType",
    "Sm",
    "Mailbox",
    "TxPdo",
    "RxPdo",
    "Dc",
    "Profile",
    "Info",
    "Image16x14",
    "ImageFile16x14",
    "Fmmu",
    "Su",
    "Eeprom",
];

/// Direct `<Eeprom>` children that the typed schema consumes and are therefore
/// NOT captured as opaque category blobs.
const KNOWN_EEPROM_CHILDREN: &[&str] = &["ByteSize", "ConfigData", "BootStrap"];

/// Strip any namespace prefix from a qualified element name (`Beckhoff:Foo` ->
/// `Foo`). Used only for the known-child membership test; the captured
/// [`RawXml::name`] keeps the qualified form quick-xml provides.
fn local_name(qualified: &str) -> &str {
    qualified.rsplit(':').next().unwrap_or(qualified)
}

/// Walk the document and, for each `<Device>` (in document order), collect a
/// `Vec<RawXml>` of its DIRECT child elements whose local name is not in
/// [`KNOWN_DEVICE_CHILDREN`]. Returns one `Vec<RawXml>` per device, in the same
/// order the serde pass produces them.
///
/// This pass is read-only and runs only after the serde deserialize already
/// succeeded, so any reader error here is unexpected; it is surfaced as
/// [`EsiError::Value`] with a located span rather than fabricating a `DeError`.
pub fn capture_device_extensions(xml: &str) -> Result<Vec<Vec<RawXml>>, EsiError> {
    let index = LineIndex::new(xml);
    let mut reader = Reader::from_str(xml);
    let mut per_device: Vec<Vec<RawXml>> = Vec::new();

    // `depth` is the nesting level of the *next* event relative to the document
    // root (root Start lands at depth 0). `device_depth` is the depth of the
    // currently-open `<Device>`, if any.
    let mut depth: i32 = 0;
    let mut device_depth: Option<i32> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(start)) => {
                let subtree_consumed = handle_start_event(
                    &start,
                    &mut reader,
                    &index,
                    depth,
                    &mut device_depth,
                    &mut per_device,
                )?;
                if !subtree_consumed {
                    depth += 1;
                }
            }
            Ok(Event::Empty(start)) => {
                handle_empty_event(
                    &start,
                    &reader,
                    &index,
                    depth,
                    device_depth,
                    &mut per_device,
                )?;
                // Empty elements do not change depth.
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if device_depth.is_some_and(|d| depth == d) {
                    // Closed the open <Device>.
                    device_depth = None;
                }
            }
            Ok(_) => {}
            Err(e) => return Err(reader_error(&reader, &index, &e)),
        }
    }

    Ok(per_device)
}

/// Handle a `Start` event in the device-extension walk.
///
/// Returns `true` when the subtree was consumed (and depth must NOT be
/// incremented by the caller), or `false` when the caller should increment
/// depth as usual.
fn handle_start_event(
    start: &quick_xml::events::BytesStart<'_>,
    reader: &mut Reader<&[u8]>,
    index: &LineIndex,
    depth: i32,
    device_depth: &mut Option<i32>,
    per_device: &mut Vec<Vec<RawXml>>,
) -> Result<bool, EsiError> {
    let name = decode_name(start, reader, index)?;
    let is_device = local_name(&name) == "Device";
    // A direct child of the open device sits one level deeper.
    let in_device_child = is_direct_device_child(*device_depth, depth);

    if in_device_child && !is_known_child(&name) {
        // Materialise the whole subtree, consuming through its End.
        let subtree = read_subtree(reader, index, start, &name)?;
        if let Some(exts) = per_device.last_mut() {
            exts.push(subtree);
        }
        // read_subtree consumed the matching End; depth is unchanged.
        return Ok(true);
    }

    if is_device && device_depth.is_none() {
        *device_depth = Some(depth);
        per_device.push(Vec::new());
    }
    Ok(false)
}

/// Handle an `Empty` event in the device-extension walk.
///
/// Captures the element as a leaf [`RawXml`] if it is an unrecognised direct
/// child of the currently-open `<Device>`. Empty elements do not change depth.
fn handle_empty_event(
    start: &quick_xml::events::BytesStart<'_>,
    reader: &Reader<&[u8]>,
    index: &LineIndex,
    depth: i32,
    device_depth: Option<i32>,
    per_device: &mut [Vec<RawXml>],
) -> Result<(), EsiError> {
    let name = decode_name(start, reader, index)?;
    if is_direct_device_child(device_depth, depth) && !is_known_child(&name) {
        let attributes = decode_attributes(start, reader, index)?;
        if let Some(exts) = per_device.last_mut() {
            exts.push(RawXml {
                name,
                attributes,
                text: None,
                children: Vec::new(),
            });
        }
    }
    Ok(())
}

/// Return `true` when `depth` is exactly one level below the open `<Device>`,
/// i.e. the current event is a direct child of that device element.
fn is_direct_device_child(device_depth: Option<i32>, depth: i32) -> bool {
    device_depth.is_some_and(|d| depth == d + 1)
}

fn is_known_child(qualified_name: &str) -> bool {
    KNOWN_DEVICE_CHILDREN.contains(&local_name(qualified_name))
}

/// Recursively materialise the element whose `Start` was just read, consuming
/// events up to and including its matching `End`. `start`/`name` describe that
/// opening tag.
fn read_subtree(
    reader: &mut Reader<&[u8]>,
    index: &LineIndex,
    start: &quick_xml::events::BytesStart<'_>,
    name: &str,
) -> Result<RawXml, EsiError> {
    let attributes = decode_attributes(start, reader, index)?;
    let mut node = RawXml {
        name: name.to_owned(),
        attributes,
        text: None,
        children: Vec::new(),
    };

    loop {
        match reader.read_event() {
            // unbalanced (Eof); serde already validated, so unreachable in practice
            Ok(Event::Eof | Event::End(_)) => break,
            Ok(Event::Start(child_start)) => {
                let child_name = decode_name(&child_start, reader, index)?;
                let child = read_subtree(reader, index, &child_start, &child_name)?;
                node.children.push(child);
            }
            Ok(Event::Empty(child_start)) => {
                let child_name = decode_name(&child_start, reader, index)?;
                let attrs = decode_attributes(&child_start, reader, index)?;
                node.children.push(RawXml {
                    name: child_name,
                    attributes: attrs,
                    text: None,
                    children: Vec::new(),
                });
            }
            Ok(Event::Text(text)) => {
                let decoded = text
                    .decode()
                    .map_err(|e| value_error(index, reader.error_position(), &e))?;
                if !decoded.trim().is_empty() {
                    node.text = Some(decoded.into_owned());
                }
            }
            Ok(_) => {}
            Err(e) => return Err(reader_error(reader, index, &e)),
        }
    }

    Ok(node)
}

/// Decode an element's name to an owned `String` (qualified, prefix kept).
fn decode_name(
    start: &quick_xml::events::BytesStart<'_>,
    reader: &Reader<&[u8]>,
    index: &LineIndex,
) -> Result<String, EsiError> {
    let qname = start.name();
    let decoder = reader.decoder();
    decoder
        .decode(qname.as_ref())
        .map(std::borrow::Cow::into_owned)
        .map_err(|e| value_error(index, reader.error_position(), &e))
}

/// Decode all attributes of an element into `(name, value)` pairs, unescaping
/// values.
fn decode_attributes(
    start: &quick_xml::events::BytesStart<'_>,
    reader: &Reader<&[u8]>,
    index: &LineIndex,
) -> Result<Vec<(String, String)>, EsiError> {
    let decoder = reader.decoder();
    let mut out = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| value_error(index, reader.error_position(), &e))?;
        let key = decoder
            .decode(attr.key.as_ref())
            .map_err(|e| value_error(index, reader.error_position(), &e))?
            .into_owned();
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| value_error(index, reader.error_position(), &e))?
            .into_owned();
        out.push((key, value));
    }
    Ok(out)
}

/// Build a located [`EsiError::Value`] from the reader's current error position.
fn reader_error(reader: &Reader<&[u8]>, index: &LineIndex, e: &quick_xml::Error) -> EsiError {
    value_error(index, reader.error_position(), e)
}

fn value_error(index: &LineIndex, byte_pos: u64, e: &dyn std::fmt::Display) -> EsiError {
    let offset = usize::try_from(byte_pos).unwrap_or(usize::MAX);
    EsiError::Value {
        path: "vendor_extensions".to_owned(),
        span: Some(index.span(offset)),
        reason: e.to_string(),
    }
}

/// Walk the document and, for each `<Device>` (in document order), collect the
/// DIRECT children of its `<Eeprom>` element whose local name is not in
/// [`KNOWN_EEPROM_CHILDREN`] (e.g. `<Category>` blocks), captured verbatim.
/// Devices without an `<Eeprom>` yield an empty vec.
pub fn capture_eeprom_categories(xml: &str) -> Result<Vec<Vec<RawXml>>, EsiError> {
    let index = LineIndex::new(xml);
    let mut reader = Reader::from_str(xml);
    let mut per_device: Vec<Vec<RawXml>> = Vec::new();

    // `depth` is the nesting level of the *next* event relative to the document
    // root (root Start lands at depth 0). Convention mirrors `capture_device_extensions`:
    // membership tests happen at the current `depth` (before increment), and
    // `device_depth` / `eeprom_depth` record the depth of the opening Start tag.
    let mut depth: i32 = 0;
    let mut device_depth: Option<i32> = None;
    let mut eeprom_depth: Option<i32> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(start)) => {
                let name = decode_name(&start, &reader, &index)?;
                let local = local_name(&name);

                // A direct child of `<Eeprom>` that is not schema-known → capture.
                if eeprom_depth.is_some_and(|d| depth == d + 1)
                    && !KNOWN_EEPROM_CHILDREN.contains(&local)
                {
                    let subtree = read_subtree(&mut reader, &index, &start, &name)?;
                    if let Some(cats) = per_device.last_mut() {
                        cats.push(subtree);
                    }
                    // read_subtree consumed the matching End; do NOT increment depth.
                    continue;
                }

                if local == "Device" && device_depth.is_none() {
                    device_depth = Some(depth);
                    per_device.push(Vec::new());
                } else if local == "Eeprom"
                    && device_depth.is_some_and(|d| depth == d + 1)
                    && eeprom_depth.is_none()
                {
                    eeprom_depth = Some(depth);
                }
                depth += 1;
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if eeprom_depth.is_some_and(|d| depth == d) {
                    eeprom_depth = None;
                }
                if device_depth.is_some_and(|d| depth == d) {
                    device_depth = None;
                }
            }
            Ok(_) => {}
            Err(e) => return Err(reader_error(&reader, &index, &e)),
        }
    }

    Ok(per_device)
}
