//! Headless orchestration of karasu-slicer jobs, shared by `karasu-cli` and `karasu-app` (M6.1).
//!
//! `karasu-core` holds every algorithm: import, repair, hollowing, supports, slicing, rasterizing
//! and export. This crate composes them into complete jobs and owns no geometry of its own:
//! - [`request`]: what one job slices, with which profiles, into which output, and how;
//! - [`pipeline`]: running a request to a published output and its summary, with progress and
//!   cancellation supplied by the caller;
//! - [`placement`]: where an object goes on the plate, shared by slicing and display;
//! - [`project`]: the project model, a library of embedded models and plates with their own
//!   printer, resin, settings and objects, with plate revisions;
//! - [`diagnosis`]: what is wrong with an imported model, and whether a plate's settings deal
//!   with it;
//! - [`jobs`]: plate requests sliced in the background, with progress events and cancellation;
//! - [`library`]: the built-in printer and resin presets and the user's profile library.
//!
//! The crate has no GUI or terminal dependency: callers render progress and report errors.
pub mod diagnosis;
pub mod jobs;
pub mod library;
pub mod logging;
pub mod pipeline;
pub mod placement;
pub mod project;
pub mod request;
