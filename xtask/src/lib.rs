//! Verification-only library shared by CI signing checks and the shipped utility.
//! No signing keys, process launching, workspace discovery or mutable state.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub mod pe_signature;
#[cfg(windows)]
pub mod windows_verification;
