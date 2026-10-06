//! Typed audit identifiers with lossless legacy UUID compatibility.

use mti::prelude::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};
use uuid::Uuid;

/// An `audit_` TypeID. New identifiers use UUIDv7; legacy UUID bits are preserved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuditEventId(MagicTypeId);

impl AuditEventId {
    /// Prefix identifying audit events.
    pub const PREFIX: &'static str = "audit";

    /// Generate a time-sortable UUIDv7 TypeID using `mti`.
    pub fn new() -> Self {
        Self(Self::PREFIX.create_type_id::<V7>())
    }

    /// Canonical public TypeID representation.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Lossless UUID storage encoding and canonical hash input.
    pub fn as_uuid(&self) -> Uuid {
        self.0.suffix().to_uuid()
    }
}

impl Default for AuditEventId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for AuditEventId {
    fn from(uuid: Uuid) -> Self {
        Self(Self::PREFIX.create_type_id_with_suffix::<V7>(uuid.into()))
    }
}

impl fmt::Display for AuditEventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An invalid audit TypeID or legacy UUID representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditEventIdError;

impl fmt::Display for AuditEventIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected an audit_ TypeID or a legacy UUID")
    }
}

impl std::error::Error for AuditEventIdError {}

impl FromStr for AuditEventId {
    type Err = AuditEventIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Ok(uuid) = Uuid::parse_str(value) {
            return Ok(uuid.into());
        }
        let id = MagicTypeId::from_str(value).map_err(|_| AuditEventIdError)?;
        if id.prefix().as_str() != Self::PREFIX {
            return Err(AuditEventIdError);
        }
        Ok(Self(id))
    }
}

impl Serialize for AuditEventId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AuditEventId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_audit_typeids_with_uuidv7() {
        let id = AuditEventId::new();
        assert!(id.as_str().starts_with("audit_"));
        assert_eq!(id.as_uuid().get_version_num(), 7);
        assert_eq!(id.as_str().parse::<AuditEventId>().unwrap(), id);
        assert_eq!(
            serde_json::from_str::<AuditEventId>(&serde_json::to_string(&id).unwrap()).unwrap(),
            id
        );
    }

    #[test]
    fn legacy_uuid_is_reconstructed_without_changing_its_bytes() {
        let uuid = Uuid::parse_str("dca93650-9d2c-4ca8-a00f-79a63467c187").unwrap();
        let id: AuditEventId = uuid.into();
        assert_eq!(id.as_uuid(), uuid);
        assert_eq!(uuid.to_string().parse::<AuditEventId>().unwrap(), id);
        assert_eq!(id.as_str().parse::<AuditEventId>().unwrap(), id);
        assert_eq!(
            serde_json::from_str::<AuditEventId>(&format!("\"{uuid}\"")).unwrap(),
            id
        );
        assert!(serde_json::to_string(&id).unwrap().starts_with("\"audit_"));
        assert!(id
            .as_str()
            .replacen("audit_", "user_", 1)
            .parse::<AuditEventId>()
            .is_err());
        assert!("audit_invalid".parse::<AuditEventId>().is_err());
    }
}
