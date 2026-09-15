//! Calibration module for inter-LLM confidence standardization
//!
//! Implements the EWMA-based sigma adjustment layer of the Double Livre pattern,
//! separating server-side confidence evaluation from agent-provided sigma values
//! without modifying signed payloads.

pub mod ewma;

pub use ewma::{EwmaCalibration, SigmaCalibrator};
