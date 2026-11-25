//! User credential management.

pub mod model;
pub mod repository;

pub use model::{NewUserCredential, UserCredential};
pub use repository::{CredentialRepository, CredentialRepositoryError};
