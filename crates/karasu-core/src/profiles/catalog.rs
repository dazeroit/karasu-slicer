//! The catalog index and catalog revisions (TD-106).
//!
//! The built-in catalog is a set of profile documents grouped by vendor, listed in one index that
//! also records what each document's values rest on. A user's library holds copies of catalog
//! entries ([`super::CatalogRef`]); [`differences`] lists what a newer catalog revision would
//! change in a copy, field by field, so the interface can offer it and never apply it unasked.
use super::{invalid, validate_profile_id, ProfileError};
use serde::{Deserialize, Serialize};

/// Schema of the catalog index.
pub const CATALOG_SCHEMA_VERSION: u32 = 1;

/// The catalog's index: its vendors and every document, by kind.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogIndex {
    /// Must equal [`CATALOG_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Vendors of printers or resins.
    pub vendors: Vec<Vendor>,
    /// Printer documents.
    pub printers: Vec<PrinterEntry>,
    /// Resin (material) documents.
    pub materials: Vec<Entry>,
    /// Print setting documents.
    pub print_settings: Vec<Entry>,
}

/// A vendor of printers or resins.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vendor {
    /// Identity, referenced by profiles' `vendor`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The vendor's website.
    pub website: String,
    /// The file formats the vendor's printers read, by format identifier, so that a custom
    /// printer of the vendor is offered those first; empty when not recorded.
    #[serde(default)]
    pub formats: Vec<String>,
    /// The vendor's own standard resin, a catalog resin: when a printer of the vendor is added
    /// from the catalog, its print settings for this resin are offered already chosen.
    #[serde(default)]
    pub standard_resin: Option<String>,
}

/// How a printer separates each cured layer from the film.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Release {
    /// The platform lifts along Z.
    ZLift,
    /// The vat tilts; Z values in a job describe only the residual head motion.
    Tilt,
}

/// A printer document and what has been verified about it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterEntry {
    /// Path under the catalog folder.
    pub file: String,
    /// How the printer releases each layer.
    pub release: Release,
    /// Whether the panel orientation was compared with the manufacturer's slicer output
    /// (TD-104).
    pub orientation_verified: bool,
    /// Source keys, as the catalog's provenance table lists them.
    pub sources: String,
}

/// A resin or print setting document.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Path under the catalog folder.
    pub file: String,
    /// Source keys, as the catalog's provenance table lists them.
    pub sources: String,
}

impl CatalogIndex {
    /// Decode and check the index's schema and vendors.
    /// # Errors
    /// Malformed JSON, an unsupported schema, or an invalid or repeated vendor identity.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProfileError> {
        let index: Self = serde_json::from_slice(bytes)?;
        if index.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(invalid(format!(
                "unsupported catalog schema version {}",
                index.schema_version
            )));
        }
        let mut ids = std::collections::BTreeSet::new();
        for vendor in &index.vendors {
            validate_profile_id(&vendor.id)?;
            if !ids.insert(vendor.id.as_str()) {
                return Err(invalid(format!("vendor {} is listed twice", vendor.id)));
            }
        }
        Ok(index)
    }
}

/// One setting a catalog revision changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Difference {
    /// The field, as a dotted path such as `limits.max_lift_mm` or `settings.normal.lift.0`.
    pub field: String,
    /// The copy's value, as JSON text (`null` when absent).
    pub ours: String,
    /// The catalog's value, as JSON text (`null` when absent).
    pub theirs: String,
}

/// Fields that identify a copy rather than describe it: never compared.
const IDENTITY: [&str; 8] = [
    "schema_version",
    "id",
    "name",
    "revision",
    "based_on",
    "printer_id",
    "resin_id",
    "status",
];

/// What the catalog's `theirs` would change in the copy `ours`, field by field, in field order.
/// Identity fields (identity, name, revision, origin, references and status) are not compared.
/// # Errors
/// A profile that cannot be represented as JSON.
pub fn differences<T: Serialize>(ours: &T, theirs: &T) -> Result<Vec<Difference>, ProfileError> {
    let ours = serde_json::to_value(ours)?;
    let theirs = serde_json::to_value(theirs)?;
    let mut out = Vec::new();
    compare("", &ours, &theirs, &mut out);
    Ok(out)
}

fn compare(
    path: &str,
    ours: &serde_json::Value,
    theirs: &serde_json::Value,
    out: &mut Vec<Difference>,
) {
    use serde_json::Value;
    let join = |key: &str| {
        if path.is_empty() {
            key.to_owned()
        } else {
            format!("{path}.{key}")
        }
    };
    match (ours, theirs) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                if path.is_empty() && IDENTITY.contains(&key.as_str()) {
                    continue;
                }
                compare(
                    &join(key),
                    a.get(key).unwrap_or(&Value::Null),
                    b.get(key).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                compare(&join(&i.to_string()), x, y, out);
            }
        }
        _ if ours != theirs => out.push(Difference {
            field: path.to_owned(),
            ours: ours.to_string(),
            theirs: theirs.to_string(),
        }),
        _ => {}
    }
}
