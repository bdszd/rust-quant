use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use thiserror::Error;

/// 密码处理错误。
#[derive(Debug, Error)]
pub enum PasswordError {
    #[error("password hashing or verification failed: {0}")]
    Hash(String),
}

/// 对明文密码进行 Argon2id 哈希。
///
/// 每次调用都会生成新的随机 Salt。
/// 返回值可以直接保存到数据库。
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    let salt = SaltString::generate(&mut OsRng);

    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| PasswordError::Hash(error.to_string()))?;

    Ok(password_hash.to_string())
}

/// 验证用户输入的密码是否与数据库中的哈希匹配。
///
/// 返回：
/// - Ok(true)：密码正确。
/// - Ok(false)：密码不匹配。
/// - Err(...)：哈希格式错误或验证过程发生其他错误。
pub fn verify_password(password: &str, encoded_hash: &str) -> Result<bool, PasswordError> {
    let parsed_hash =
        PasswordHash::new(encoded_hash).map_err(|error| PasswordError::Hash(error.to_string()))?;

    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),

        // 密码不匹配属于正常的认证失败，不是程序错误。
        Err(argon2::password_hash::Error::Password) => Ok(false),

        Err(error) => Err(PasswordError::Hash(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_can_be_hashed_and_verified() {
        let password = "MyPassword123";

        let hash = hash_password(password).unwrap();

        assert_ne!(hash, password);
        assert!(verify_password(password, &hash).unwrap());
    }

    #[test]
    fn wrong_password_should_not_match() {
        let hash = hash_password("MyPassword123").unwrap();

        assert!(!verify_password("WrongPassword123", &hash).unwrap());
    }

    #[test]
    fn same_password_should_produce_different_hashes() {
        let password = "MyPassword123";

        let hash1 = hash_password(password).unwrap();
        let hash2 = hash_password(password).unwrap();

        assert_ne!(hash1, hash2);

        assert!(verify_password(password, &hash1).unwrap());
        assert!(verify_password(password, &hash2).unwrap());
    }
}
