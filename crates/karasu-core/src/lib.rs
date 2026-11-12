//! Reusable core types and processing facilities for karasu-slicer.
//!
//! Mesh coordinates use millimeters. STL import requires an explicit unit policy and preserves
//! source geometry without placement, repair, or scene transforms. A local triangle–plane section
//! kernel, raw single-plane traversal and provenance-aware incidence normalization are available.
//! A derived incidence graph separates complete and locally eligible adjacency. Conservative ordered
//! path extraction retains open chains, provisional cycles and ambiguity. Separate exact planar
//! inspection, cycle area/orientation, exact-grid mapping and strict region hierarchy are available.
//! Explicit uniform layer plans use exact dyadic membership and once-rounded finite plane values;
//! the owned layer engine composes those stages with fused errors, advisory progress and optional
//! atomic collection. Explicit source-material composition constructs exact rational boundaries;
//! bounded LCD rasterization and reference PNG export are available separately.
//! Preparation offers convex hollowing, drainage and supports, plus sampled-distance hollowing of
//! arbitrary closed solids (`preparation::sampled`); `digest` provides SHA-256 asset identity.
//! Separate validated object/affine transforms and tight transformed-vertex bounds are available
//! without mutating those source meshes or their STL metadata.

pub mod analysis;
pub mod archive;
mod boolean;
pub mod cancellation;
pub mod digest;
pub mod export;
pub mod geometry;
pub mod layer_plan;
pub mod mesh;
mod parallel;
pub mod plate;
pub mod preparation;
pub mod preview;
pub mod print_job;
pub mod profiles;
pub mod raster;
pub mod slicing;
pub mod supports;

/// The human-readable product name.
pub const APPLICATION_NAME: &str = "karasu-slicer";

/// A short description suitable for command-line help.
pub const APPLICATION_DESCRIPTION: &str = "A native Rust MSLA/LCD resin slicing application";

/// The copyright notice (owner, 2026-11-11).
pub const COPYRIGHT: &str = "Copyright (C) 2026 Ivan Maruca";

/// The licence's SPDX identifier (owner, 2026-11-11): the GNU Affero General Public License,
/// version 3 or any later version. The name "karasu-slicer" and its logo are trademarks, not
/// licensed under it (see `TRADEMARKS.md`).
pub const LICENCE: &str = "AGPL-3.0-or-later";

/// Where the licence's text is published.
pub const LICENCE_URL: &str = "https://www.gnu.org/licenses/agpl-3.0.html";

/// Where the program's corresponding source is published.
pub const SOURCE_URL: &str = "https://github.com/dazeroit/karasu-slicer";

/// The notice an interactive interface shows (the licence's "Appropriate Legal Notices"): the
/// licence, the absence of warranty and where the source is.
pub const LEGAL_NOTICE: &str = "This program is free software: you can redistribute it and/or \
modify it under the terms of the GNU Affero General Public License as published by the Free \
Software Foundation, either version 3 of the License, or (at your option) any later version. It \
is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the \
implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero \
General Public License for more details.";

#[cfg(test)]
mod tests {
    use super::{APPLICATION_DESCRIPTION, APPLICATION_NAME, LEGAL_NOTICE, LICENCE, SOURCE_URL};

    #[test]
    fn application_identity_is_stable() {
        assert_eq!(APPLICATION_NAME, "karasu-slicer");
        assert_ne!(APPLICATION_DESCRIPTION, "");
        assert_eq!(LICENCE, "AGPL-3.0-or-later");
        assert!(LEGAL_NOTICE.contains("GNU Affero General Public License"));
        assert!(LEGAL_NOTICE.contains("WITHOUT ANY WARRANTY"));
        assert!(SOURCE_URL.starts_with("https://"));
    }

    #[test]
    fn the_licence_file_is_the_agpl_and_the_manifest_names_it() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(root.join("LICENSE")).unwrap();
        assert!(text.starts_with("                    GNU AFFERO GENERAL PUBLIC LICENSE"));
        assert!(text.contains("Version 3, 19 November 2007"));
        assert!(text
            .contains("13. Remote Network Interaction; Use with the GNU General Public License."));
        assert_eq!(env!("CARGO_PKG_LICENSE"), LICENCE);
    }
}
