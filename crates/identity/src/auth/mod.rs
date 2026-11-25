//! Authentication logic.

pub mod login;
pub mod password;
pub mod registration;
pub mod service;

pub use login::{LoginError, LoginService};

pub use password::{PasswordError, hash_password, verify_password};

pub use registration::{RegistrationError, RegistrationService};

pub use service::{AuthenticationError, AuthenticationService};
