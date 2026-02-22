//! Terminal UI layer for Claude Monitor.
//!
//! Provides themes, progress bars, header, indicator components, session and
//! table views, and the main application event loop built on top of
//! [`ratatui`] for rendering usage dashboards in the terminal.

pub mod app;
pub mod components;
pub mod session_view;
pub mod table_view;
pub mod themes;

pub use monitor_core as core;
