//! Windows certificate-store verification and signing over the portable signature binding in
//! `suite42_app::pdf::signatures`.
pub use suite42_app::pdf::signatures::*;
mod windows;
pub use windows::{SignWorker, Worker};
