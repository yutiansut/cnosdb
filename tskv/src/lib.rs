#![allow(dead_code)]
#![allow(unreachable_patterns)]

use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
pub use compaction::check::vnode_table_checksum_schema;
use datafusion::arrow::record_batch::RecordBatch;
use models::meta_data::VnodeId;
use models::predicate::domain::{ColumnDomains, TimeRange};
use models::schema::{Precision, TableColumn};
use models::{ColumnId, SeriesId, SeriesKey};
use protos::kv_service::{WritePointsRequest, WritePointsResponse};
use trace::SpanContext;

pub use crate::error::{Error, Result};
pub use crate::kv_option::Options;
use crate::kv_option::StorageOptions;
pub use crate::kvcore::TsKv;
pub use crate::summary::{print_summary_statistics, Summary, VersionEdit};
use crate::tseries_family::SuperVersion;
pub use crate::tsm::print_tsm_statistics;
pub use crate::wal::print_wal_statistics;

pub mod byte_utils;
mod compaction;
mod compute;
mod context;
pub mod database;
pub mod engine_mock;
pub mod error;
pub mod file_system;
pub mod file_utils;
pub mod index;
pub mod kv_option;
mod kvcore;
mod memcache;
// TODO supposedly private
pub mod reader;
mod record_file;
mod schema;
mod summary;
mod tseries_family;
pub mod tsm;
mod version_set;
mod wal;

pub type ColumnFileId = u64;
type TseriesFamilyId = u32;
type LevelId = u32;

pub fn tenant_name_from_request(req: &protos::kv_service::WritePointsRequest) -> String {
    match &req.meta {
        Some(meta) => meta.tenant.clone(),
        None => models::schema::DEFAULT_CATALOG.to_string(),
    }
}

pub type EngineRef = Arc<dyn Engine>;

#[async_trait]
pub trait Engine: Send + Sync + Debug {
    async fn write(
        &self,
        span_ctx: Option<&SpanContext>,
        id: u32,
        precision: Precision,
        write_batch: WritePointsRequest,
    ) -> Result<WritePointsResponse>;

    async fn write_from_wal(
        &self,
        id: u32,
        precision: Precision,
        write_batch: WritePointsRequest,
        seq: u64,
    ) -> Result<()>;

    async fn drop_database(&self, tenant: &str, database: &str) -> Result<()>;

    async fn drop_table(&self, tenant: &str, database: &str, table: &str) -> Result<()>;

    async fn remove_tsfamily(&self, tenant: &str, database: &str, id: u32) -> Result<()>;

    async fn prepare_copy_vnode(&self, tenant: &str, database: &str, vnode_id: u32) -> Result<()>;
    async fn flush_tsfamily(&self, tenant: &str, database: &str, id: u32) -> Result<()>;

    async fn add_table_column(
        &self,
        tenant: &str,
        database: &str,
        table: &str,
        column: TableColumn,
    ) -> Result<()>;

    async fn drop_table_column(
        &self,
        tenant: &str,
        database: &str,
        table: &str,
        column: &str,
    ) -> Result<()>;

    async fn change_table_column(
        &self,
        tenant: &str,
        database: &str,
        table: &str,
        column_name: &str,
        new_column: TableColumn,
    ) -> Result<()>;

    async fn delete_series(
        &self,
        tenant: &str,
        database: &str,
        series_ids: &[SeriesId],
        field_ids: &[ColumnId],
        time_range: &TimeRange,
    ) -> Result<()>;

    async fn get_series_id_by_filter(
        &self,
        tenant: &str,
        db: &str,
        tab: &str,
        vnode_id: VnodeId,
        filter: &ColumnDomains<String>,
    ) -> Result<Vec<SeriesId>>;

    async fn get_series_key(
        &self,
        tenant: &str,
        db: &str,
        vnode_id: u32,
        sid: SeriesId,
    ) -> Result<Option<SeriesKey>>;

    async fn get_db_version(
        &self,
        tenant: &str,
        db: &str,
        vnode_id: u32,
    ) -> Result<Option<Arc<SuperVersion>>>;

    fn get_storage_options(&self) -> Arc<StorageOptions>;

    async fn get_vnode_summary(
        &self,
        tenant: &str,
        database: &str,
        vnode_id: u32,
    ) -> Result<Option<VersionEdit>>;

    async fn apply_vnode_summary(
        &self,
        tenant: &str,
        database: &str,
        vnode_id: u32,
        summary: VersionEdit,
    ) -> Result<()>;

    async fn drop_vnode(&self, id: TseriesFamilyId) -> Result<()>;

    async fn compact(&self, vnode_ids: Vec<TseriesFamilyId>) -> Result<()>;

    async fn get_vnode_hash_tree(&self, vnode_id: VnodeId) -> Result<RecordBatch>;

    async fn close(&self);
}
