//! Compatibility audit backend.
use super::{AuditQuery, AuditStorage, AuditVerification};
use crate::audit::event::AuditEvent;
use crate::error::Error;
/// Compatibility backend preserving the service error type.
pub struct MssqlAuditStorage(acton_service_mssql::storage::MssqlAuditStorage);
impl MssqlAuditStorage {
    /// Create audit storage from an established connection.
    pub fn new(connection: crate::mssql::MssqlPool) -> Self {
        Self(acton_service_mssql::storage::MssqlAuditStorage::new(
            connection,
        ))
    }
    /// Initialize the backend schema and append-only protections.
    pub async fn initialize(&self) -> Result<(), Error> {
        self.0.initialize().await.map_err(Into::into)
    }
}
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
impl AuditStorage for MssqlAuditStorage {
    async fn append(&self, event: &AuditEvent) -> Result<(), Error> {
        acton_service_audit::storage::AuditStorage::append(&self.0, event)
            .await
            .map_err(Into::into)
    }
    async fn query_filtered(&self, _query: &AuditQuery) -> Result<Vec<AuditEvent>, Error> {
        acton_service_audit::storage::AuditStorage::query_filtered(&self.0, &_query.into())
            .await
            .map_err(Into::into)
    }
    async fn latest(&self) -> Result<Option<AuditEvent>, Error> {
        acton_service_audit::storage::AuditStorage::latest(&self.0)
            .await
            .map_err(Into::into)
    }
    async fn query_range(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        acton_service_audit::storage::AuditStorage::query_range(&self.0, from, to, limit)
            .await
            .map_err(Into::into)
    }
    async fn query_sequence(
        &self,
        _from: u64,
        _to: u64,
        _limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        acton_service_audit::storage::AuditStorage::query_sequence(&self.0, _from, _to, _limit)
            .await
            .map_err(Into::into)
    }
    async fn sequence_bounds(&self) -> Result<Option<(u64, u64)>, Error> {
        acton_service_audit::storage::AuditStorage::sequence_bounds(&self.0)
            .await
            .map_err(Into::into)
    }
    async fn verify_chain_range(&self, from: u64, to: u64) -> Result<AuditVerification, Error> {
        acton_service_audit::storage::AuditStorage::verify_chain_range(&self.0, from, to)
            .await
            .map_err(Into::into)
    }
    async fn verify_chain(&self, from_sequence: u64) -> Result<Option<u64>, Error> {
        acton_service_audit::storage::AuditStorage::verify_chain(&self.0, from_sequence)
            .await
            .map_err(Into::into)
    }
    async fn query_before(
        &self,
        _cutoff: DateTime<Utc>,
        _limit: usize,
    ) -> Result<Vec<AuditEvent>, Error> {
        acton_service_audit::storage::AuditStorage::query_before(&self.0, _cutoff, _limit)
            .await
            .map_err(Into::into)
    }
    async fn purge_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, Error> {
        acton_service_audit::storage::AuditStorage::purge_before(&self.0, _cutoff)
            .await
            .map_err(Into::into)
    }
    async fn ensure_ready(&self) -> Result<(), Error> {
        acton_service_audit::storage::AuditStorage::ensure_ready(&self.0)
            .await
            .map_err(Into::into)
    }
}
#[async_trait]
impl super::lazy::InitializableStorage for MssqlAuditStorage {
    type Conn = <acton_service_mssql::storage::MssqlAuditStorage as acton_service_audit::storage::lazy::InitializableStorage>::Conn;
    fn from_conn(conn: Self::Conn) -> Self {
        Self(<acton_service_mssql::storage::MssqlAuditStorage as acton_service_audit::storage::lazy::InitializableStorage>::from_conn(conn))
    }
    async fn init_schema(&self) -> Result<(), Error> {
        acton_service_audit::storage::lazy::InitializableStorage::init_schema(&self.0)
            .await
            .map_err(Into::into)
    }
    fn backend_name() -> &'static str {
        <acton_service_mssql::storage::MssqlAuditStorage as acton_service_audit::storage::lazy::InitializableStorage>::backend_name()
    }
}
