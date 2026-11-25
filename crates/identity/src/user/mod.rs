//! User management.

pub mod model;
pub mod repository;

pub use model::{NewUser, User, UserStatus};
pub use repository::{UserRepository, UserRepositoryError};
