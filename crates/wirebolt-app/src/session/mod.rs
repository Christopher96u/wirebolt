//! Open documents, editor groups and tabs, runs and their streaming state, and the
//! orchestration of workspace edits, sends, history, undo and secrets.
pub mod persistence;
pub mod run;
pub mod secrets;
pub mod tabs;
#[cfg(test)]
mod tests;
pub mod transport;
pub mod websocket;
pub mod workspace;

pub mod history;
pub mod socket_transport;
