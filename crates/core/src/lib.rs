//! Stormwaves core: a stormwater hydrology and hydraulics engine.
//!
//! Everything here is UI-independent and compiles to WebAssembly. Internal units are US
//! customary: feet, square feet, cubic feet, cfs, hours, and inches of rainfall; conversion to
//! display units happens at the edges.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cn;
pub mod geometry;
pub mod hydrograph;
pub mod outlets;
pub mod rainfall;
mod rainfall_data;
pub mod runoff;
pub mod storage;
pub mod tc;
pub mod uh;
