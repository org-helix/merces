//! Merces domain types shared by every crate: the agents, what they produce,
//! the trade plans, the paper-tracked signals and the events the engine sends to the UI.

mod agent;
mod event;
mod market;
mod report;
mod signal;

pub use agent::*;
pub use event::*;
pub use market::*;
pub use report::*;
pub use signal::*;
