//! Runtime orchestration layer for Claude Monitor.
//!
//! Coordinates the data-ingestion and UI layers, manages the event loop,
//! and handles configuration loading.

pub mod data_manager;
pub mod orchestrator;
pub mod session_monitor;

pub use monitor_core as core;
pub use monitor_data as data;
