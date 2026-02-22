//! Data ingestion layer for Claude Monitor.
//!
//! Responsible for discovering, reading, and parsing JSONL usage files
//! produced by the Claude CLI, building session blocks, aggregating statistics
//! and running the top-level analysis pipeline.

pub mod aggregator;
pub mod analysis;
pub mod analyzer;
pub mod reader;

pub use monitor_core as core;
