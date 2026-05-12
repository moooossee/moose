pub mod chat;
pub mod conversations;
pub mod core;
pub mod error;
pub mod ollama;
pub mod platform;
pub mod providers;
pub mod runtime;
pub mod storage;

#[cfg(feature = "gui")]
pub mod ui;

pub const APPLICATION_ID: &str = "io.github.moooossee.Moose";
pub const APPLICATION_NAME: &str = "Moose";
