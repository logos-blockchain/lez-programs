#[cfg(feature = "local-sequencer-tests")]
mod local_sequencer;

#[cfg(feature = "local-sequencer-tests")]
pub use local_sequencer::TestState;

#[cfg(not(feature = "local-sequencer-tests"))]
pub type TestState = nssa::V03State;
