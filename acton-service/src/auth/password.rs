//! Password hashing using Argon2id
//!
//! Provides secure password hashing following OWASP recommendations.
//! Uses Argon2id, which is the recommended algorithm for password hashing.
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::auth::PasswordHasher;
//!
//! let hasher = PasswordHasher::default();
//!
//! // Hash a password
//! let hash = hasher.hash("my_secure_password")?;
//!
//! // Verify a password
//! assert!(hasher.verify("my_secure_password", &hash)?);
//! assert!(!hasher.verify("wrong_password", &hash)?);
//! ```

use argon2::{
    password_hash::{
        rand_core::OsRng, PasswordHash, PasswordHasher as Argon2Hasher, PasswordVerifier,
        SaltString,
    },
    Algorithm, Argon2, Params, Version,
};

use crate::auth::config::PasswordConfig;
use crate::error::Error;

/// Password hasher using Argon2id
///
/// This hasher uses Argon2id with OWASP-recommended parameters by default.
/// The parameters can be customized via `PasswordConfig`.
#[derive(Clone)]
pub struct PasswordHasher {
    params: Params,
    min_password_length: usize,
}

impl Default for PasswordHasher {
    fn default() -> Self {
        Self::new(PasswordConfig::default())
    }
}

impl PasswordHasher {
    /// Create a new password hasher with custom configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Password hashing configuration
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use acton_service::auth::{PasswordHasher, PasswordConfig};
    ///
    /// let config = PasswordConfig {
    ///     memory_cost_kib: 65536,
    ///     time_cost: 3,
    ///     parallelism: 4,
    ///     min_password_length: 12,
    /// };
    /// let hasher = PasswordHasher::new(config);
    /// ```
    pub fn new(config: PasswordConfig) -> Self {
        let params = Params::new(
            config.memory_cost_kib,
            config.time_cost,
            config.parallelism,
            None, // Use default output length
        )
        .expect("Invalid Argon2 parameters");

        Self {
            params,
            min_password_length: config.min_password_length,
        }
    }

    /// Hash a password
    ///
    /// Returns a PHC string format hash that includes the algorithm,
    /// parameters, salt, and hash value. This format is self-describing
    /// and can be used for verification without additional context.
    ///
    /// # Arguments
    ///
    /// * `password` - The plaintext password to hash
    ///
    /// # Returns
    ///
    /// A PHC string format hash on success, or an error if:
    /// - The password is too short (less than `min_password_length`)
    /// - There's a cryptographic error during hashing
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let hasher = PasswordHasher::default();
    /// let hash = hasher.hash("secure_password_123")?;
    /// // hash looks like: $argon2id$v=19$m=65536,t=3,p=4$...
    /// ```
    pub fn hash(&self, password: &str) -> Result<String, Error> {
        // Validate password length
        if password.len() < self.min_password_length {
            return Err(Error::ValidationError(format!(
                "Password must be at least {} characters",
                self.min_password_length
            )));
        }

        // Generate a random salt
        let salt = SaltString::generate(&mut OsRng);

        // Create Argon2 instance with our parameters
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, self.params.clone());

        // Hash the password
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| Error::Auth(format!("Failed to hash password: {}", e)))?;

        Ok(hash.to_string())
    }

    /// Verify a password against a hash
    ///
    /// Uses constant-time comparison to prevent timing attacks.
    ///
    /// # Arguments
    ///
    /// * `password` - The plaintext password to verify
    /// * `hash` - The PHC string format hash to verify against
    ///
    /// # Returns
    ///
    /// `true` if the password matches, `false` otherwise.
    /// Returns an error if the hash format is invalid.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let hasher = PasswordHasher::default();
    /// let hash = hasher.hash("my_password")?;
    ///
    /// assert!(hasher.verify("my_password", &hash)?);
    /// assert!(!hasher.verify("wrong_password", &hash)?);
    /// ```
    pub fn verify(&self, password: &str, hash: &str) -> Result<bool, Error> {
        // Parse the hash
        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| Error::Auth(format!("Invalid password hash format: {}", e)))?;

        // Create Argon2 instance (parameters are read from the hash)
        let argon2 = Argon2::default();

        // Verify using constant-time comparison
        match argon2.verify_password(password.as_bytes(), &parsed_hash) {
            Ok(()) => Ok(true),
            Err(argon2::password_hash::Error::Password) => Ok(false),
            Err(e) => Err(Error::Auth(format!("Password verification failed: {}", e))),
        }
    }

    /// Check if a hash needs rehashing
    ///
    /// This is useful when you want to upgrade hashes to new parameters
    /// without requiring users to change their passwords.
    ///
    /// # Arguments
    ///
    /// * `hash` - The PHC string format hash to check
    ///
    /// # Returns
    ///
    /// `true` if the hash was created with different parameters than
    /// the current configuration, `false` if it matches.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let old_hasher = PasswordHasher::new(old_config);
    /// let hash = old_hasher.hash("password")?;
    ///
    /// let new_hasher = PasswordHasher::new(new_config);
    /// if new_hasher.needs_rehash(&hash) {
    ///     // Re-hash with new parameters on next login
    /// }
    /// ```
    pub fn needs_rehash(&self, hash: &str) -> bool {
        let Ok(parsed_hash) = PasswordHash::new(hash) else {
            return true; // Invalid hash format, should rehash
        };

        // Check algorithm
        if parsed_hash.algorithm != argon2::Algorithm::Argon2id.ident() {
            return true;
        }

        // Check version
        let Some(version) = parsed_hash.version else {
            return true;
        };
        if version != 19 {
            // Version 0x13 = 19
            return true;
        }

        // Check parameters from the hash
        let params = &parsed_hash.params;

        // Get m, t, p parameters
        let m = params
            .iter()
            .find(|(k, _)| k.as_str() == "m")
            .and_then(|(_, v)| v.decimal().ok());
        let t = params
            .iter()
            .find(|(k, _)| k.as_str() == "t")
            .and_then(|(_, v)| v.decimal().ok());
        let p = params
            .iter()
            .find(|(k, _)| k.as_str() == "p")
            .and_then(|(_, v)| v.decimal().ok());

        // Compare with our parameters
        if m != Some(self.params.m_cost()) {
            return true;
        }
        if t != Some(self.params.t_cost()) {
            return true;
        }
        if p != Some(self.params.p_cost()) {
            return true;
        }

        false
    }

    /// Get the minimum password length requirement
    pub fn min_password_length(&self) -> usize {
        self.min_password_length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let hasher = PasswordHasher::default();
        let password = "test_password_123";

        let hash = hasher.hash(password).expect("Failed to hash password");
        assert!(hash.starts_with("$argon2id$"));

        assert!(hasher.verify(password, &hash).expect("Verification failed"));
        assert!(!hasher
            .verify("wrong_password", &hash)
            .expect("Verification failed"));
    }

    #[test]
    fn test_password_too_short() {
        let hasher = PasswordHasher::default();
        let result = hasher.hash("short");

        assert!(result.is_err());
        if let Err(Error::ValidationError(msg)) = result {
            assert!(msg.contains("at least 8 characters"));
        } else {
            panic!("Expected ValidationError");
        }
    }

    #[test]
    fn test_custom_min_length() {
        let config = PasswordConfig {
            min_password_length: 12,
            ..Default::default()
        };
        let hasher = PasswordHasher::new(config);

        // 10 chars - too short
        assert!(hasher.hash("0123456789").is_err());

        // 12 chars - ok
        assert!(hasher.hash("012345678901").is_ok());
    }

    #[test]
    fn test_needs_rehash_same_params() {
        let hasher = PasswordHasher::default();
        let hash = hasher.hash("test_password_123").unwrap();

        assert!(!hasher.needs_rehash(&hash));
    }

    #[test]
    fn test_needs_rehash_different_params() {
        let hasher1 = PasswordHasher::new(PasswordConfig {
            memory_cost_kib: 32768,
            ..Default::default()
        });
        let hash = hasher1.hash("test_password_123").unwrap();

        let hasher2 = PasswordHasher::new(PasswordConfig {
            memory_cost_kib: 65536,
            ..Default::default()
        });

        assert!(hasher2.needs_rehash(&hash));
    }

    #[test]
    fn test_invalid_hash_format() {
        let hasher = PasswordHasher::default();
        let result = hasher.verify("password", "not_a_valid_hash");

        assert!(result.is_err());
    }

    #[test]
    fn test_different_hashes_for_same_password() {
        let hasher = PasswordHasher::default();
        let password = "test_password_123";

        let hash1 = hasher.hash(password).unwrap();
        let hash2 = hasher.hash(password).unwrap();

        // Different salts = different hashes
        assert_ne!(hash1, hash2);

        // But both should verify
        assert!(hasher.verify(password, &hash1).unwrap());
        assert!(hasher.verify(password, &hash2).unwrap());
    }
}
