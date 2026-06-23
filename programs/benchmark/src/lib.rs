//! Cycle benchmarks for LEZ programs.
//!
//! This crate has no library code of its own; it exists solely as a host for the benchmarks under
//! `tests/`, which run the real guest ELFs through the RISC Zero executor to measure zkVM cycle
//! costs. It is excluded from the repo workspace so normal builds/tests/CI never compile it.
