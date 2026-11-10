//! Recording oracle fixture for AMM integration tests, never a production price source.

pub mod oracle;
include!(concat!(env!("OUT_DIR"), "/methods.rs"));
