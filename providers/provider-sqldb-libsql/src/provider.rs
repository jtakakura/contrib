#![cfg(not(doctest))]

//! SQL-powered database access provider implementing `wasmcloud:libsql` for connecting
//! to libSQL clusters.
//!
//! This implementation is multi-threaded and operations between different actors
//! use different connections and can run in parallel.
//!

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use deadpool_libsql::Pool;
use libsql::params_from_iter;
use tokio::sync::RwLock;
use tracing::{error, instrument, warn};

use wasmcloud_provider_sdk::{
    get_connection, initialize_observability, propagate_trace_for_ctx, run_provider,
    serve_provider_exports, Context, LinkConfig, LinkDeleteInfo, Provider,
};

use crate::bindings::types::{LibsqlValue, QueryError, ResultRow, ResultRowEntry};
use crate::bindings::{execute, query, serve};
use crate::config::{extract_prefixed_conn_config, ConnectionCreateOptions};

/// A unique identifier for a created connection
type SourceId = String;

#[derive(Clone, Default)]
pub struct LibsqlProvider {
    /// Database connections indexed by source ID name
    connections: Arc<RwLock<HashMap<SourceId, Pool>>>,
}

impl LibsqlProvider {
    fn name() -> &'static str {
        "sqldb-libsql-provider"
    }

    /// Run [`LibsqlProvider`] as a wasmCloud provider
    pub async fn run() -> anyhow::Result<()> {
        initialize_observability!(
            LibsqlProvider::name(),
            std::env::var_os("PROVIDER_SQLDB_LIBSQL_FLAMEGRAPH_PATH")
        );
        let provider = LibsqlProvider::default();
        let shutdown = run_provider(provider.clone(), LibsqlProvider::name())
            .await
            .context("failed to run provider")?;
        let connection = get_connection();
        let wrpc = connection
            .get_wrpc_client(connection.provider_key())
            .await?;
        serve_provider_exports(&wrpc, provider, shutdown, serve)
            .await
            .context("failed to serve provider exports")
    }

    /// Create and store a connection pool, if not already present
    async fn ensure_pool(
        &self,
        source_id: &str,
        create_opts: ConnectionCreateOptions,
    ) -> Result<()> {
        // Exit early if a pool with the given source ID is already present
        {
            let connections = self.connections.read().await;
            if connections.get(source_id).is_some() {
                return Ok(());
            }
        }

        // Build the new connection pool
        let runtime = Some(deadpool_libsql::Runtime::Tokio1);
        let config = deadpool_libsql::Config::from(create_opts);
        let pool = config
            .create_pool(runtime)
            .await
            .context("failed to create connection pool")?;

        // Save the newly created connection to the pool
        let mut connections = self.connections.write().await;
        connections.insert(source_id.into(), pool);
        Ok(())
    }

    /// Execute a query
    async fn do_execute(
        &self,
        source_id: &str,
        query: &str,
        params: Vec<LibsqlValue>,
    ) -> Result<u64, QueryError> {
        let connections = self.connections.read().await;
        let pool = connections.get(source_id).ok_or_else(|| {
            QueryError::Unexpected(format!(
                "missing connection pool for source [{source_id}] while executing"
            ))
        })?;

        let connection = pool.get().await.map_err(|e| {
            QueryError::Unexpected(format!("failed to build connection from pool: {e}"))
        })?;

        let result = connection
            .execute(query, params_from_iter(params))
            .await
            .map_err(|e| QueryError::Unexpected(format!("failed to perform execute: {e}")))?;

        Ok(result)
    }

    /// Execute a batch of queries
    async fn do_execute_batch(
        &self,
        source_id: &str,
        query: &str,
    ) -> Result<Vec<ResultRow>, QueryError> {
        let connections = self.connections.read().await;
        let pool = connections.get(source_id).ok_or_else(|| {
            QueryError::Unexpected(format!(
                "missing connection pool for source [{source_id}] while executing"
            ))
        })?;

        let connection = pool.get().await.map_err(|e| {
            QueryError::Unexpected(format!("failed to build connection from pool: {e}"))
        })?;

        let batch_rows = connection
            .execute_batch(query)
            .await
            .map_err(|e| QueryError::Unexpected(format!("failed to perform execute: {e}")))?;

        // Convert rows to the expected format
        Ok(batch_rows_to_result_rows(batch_rows).await)
    }

    /// Execute a batch of queries in a transaction
    async fn do_execute_transactional_batch(
        &self,
        source_id: &str,
        query: &str,
    ) -> Result<Vec<ResultRow>, QueryError> {
        let connections = self.connections.read().await;
        let pool = connections.get(source_id).ok_or_else(|| {
            QueryError::Unexpected(format!(
                "missing connection pool for source [{source_id}] while executing transaction"
            ))
        })?;

        let connection = pool.get().await.map_err(|e| {
            QueryError::Unexpected(format!("failed to build connection from pool: {e}"))
        })?;

        let batch_rows = connection
            .execute_transactional_batch(query)
            .await
            .map_err(|e| QueryError::Unexpected(format!("failed to perform execute: {e}")))?;

        // Convert rows to the expected format
        Ok(batch_rows_to_result_rows(batch_rows).await)
    }

    /// Perform a query
    async fn do_query(
        &self,
        source_id: &str,
        query: &str,
        params: Vec<LibsqlValue>,
    ) -> Result<Vec<ResultRow>, QueryError> {
        let connections = self.connections.read().await;
        let pool = connections.get(source_id).ok_or_else(|| {
            QueryError::Unexpected(format!(
                "missing connection pool for source [{source_id}] while querying"
            ))
        })?;

        let connection = pool.get().await.map_err(|e| {
            QueryError::Unexpected(format!("failed to build connection from pool: {e}"))
        })?;

        let rows = connection
            .query(query, params_from_iter(params))
            .await
            .map_err(|e| QueryError::Unexpected(format!("failed to perform query: {e}")))?;

        // Convert rows to the expected format
        Ok(rows_to_result_rows(rows).await)
    }

    async fn do_last_insert_rowid(&self, source_id: &str) -> Result<i64, QueryError> {
        let connections = self.connections.read().await;
        let pool = connections.get(source_id).ok_or_else(|| {
            QueryError::Unexpected(format!(
                "missing connection pool for source [{source_id}] while getting last insert row ID"
            ))
        })?;

        let connection = pool.get().await.map_err(|e| {
            QueryError::Unexpected(format!("failed to build connection from pool: {e}"))
        })?;

        Ok(connection.last_insert_rowid())
    }
}

impl Provider for LibsqlProvider {
    /// Handle being linked to a source (likely a component) as a target
    ///
    /// Components are expected to provide references to named configuration via link definitions
    /// which contain keys named `LIBSQL_*` detailing configuration for connecting to libSQL.
    #[instrument(level = "debug", skip_all, fields(source_id))]
    async fn receive_link_config_as_target(
        &self,
        link_config @ LinkConfig { source_id, .. }: LinkConfig<'_>,
    ) -> anyhow::Result<()> {
        // Attempt to parse a configuration from the map with the prefix LIBSQL_
        let Some(db_cfg) = extract_prefixed_conn_config("LIBSQL_", &link_config) else {
            // If we failed to find a config on the link, then we
            warn!(source_id, "no link-level DB configuration");
            return Ok(());
        };

        // Create a pool if one isn't already present for this particular source
        if let Err(error) = self.ensure_pool(source_id, db_cfg).await {
            error!(?error, source_id, "failed to create connection");
        };

        Ok(())
    }

    /// Handle notification that a link is dropped
    ///
    /// Generally we can release the resources (connections) associated with the source
    #[instrument(level = "info", skip_all, fields(source_id = info.get_source_id()))]
    async fn delete_link_as_target(&self, info: impl LinkDeleteInfo) -> anyhow::Result<()> {
        let source_id = info.get_source_id();
        let mut connections = self.connections.write().await;
        connections.remove(source_id);
        drop(connections);
        Ok(())
    }

    /// Handle shutdown request by closing all connections
    #[instrument(level = "debug", skip_all)]
    async fn shutdown(&self) -> anyhow::Result<()> {
        let mut connections = self.connections.write().await;
        connections.drain();
        Ok(())
    }
}

impl execute::Handler<Option<Context>> for LibsqlProvider {
    #[instrument(level = "debug", skip_all, fields(query))]
    async fn execute(
        &self,
        ctx: Option<Context>,
        query: String,
        params: Vec<LibsqlValue>,
    ) -> Result<Result<u64, QueryError>> {
        propagate_trace_for_ctx!(ctx);
        let Some(Context {
            component: Some(source_id),
            ..
        }) = ctx
        else {
            return Ok(Err(QueryError::Unexpected(
                "unexpectedly missing source ID".into(),
            )));
        };

        Ok(self.do_execute(&source_id, &query, params).await)
    }

    #[instrument(level = "debug", skip_all, fields(query))]
    async fn execute_batch(
        &self,
        ctx: Option<Context>,
        query: String,
    ) -> Result<Result<Vec<ResultRow>, QueryError>> {
        propagate_trace_for_ctx!(ctx);
        let Some(Context {
            component: Some(source_id),
            ..
        }) = ctx
        else {
            return Ok(Err(QueryError::Unexpected(
                "unexpectedly missing source ID".into(),
            )));
        };

        Ok(self.do_execute_batch(&source_id, &query).await)
    }

    #[instrument(level = "debug", skip_all, fields(query))]
    async fn execute_transactional_batch(
        &self,
        ctx: Option<Context>,
        query: String,
    ) -> Result<Result<Vec<ResultRow>, QueryError>> {
        propagate_trace_for_ctx!(ctx);
        let Some(Context {
            component: Some(source_id),
            ..
        }) = ctx
        else {
            return Ok(Err(QueryError::Unexpected(
                "unexpectedly missing source ID".into(),
            )));
        };

        Ok(self
            .do_execute_transactional_batch(&source_id, &query)
            .await)
    }

    #[instrument(level = "debug", skip_all)]
    async fn last_insert_rowid(&self, ctx: Option<Context>) -> Result<Result<i64, QueryError>> {
        propagate_trace_for_ctx!(ctx);
        let Some(Context {
            component: Some(source_id),
            ..
        }) = ctx
        else {
            return Ok(Err(QueryError::Unexpected(
                "unexpectedly missing source ID".into(),
            )));
        };

        Ok(self.do_last_insert_rowid(&source_id).await)
    }
}

// Implement the required Handler trait for LibsqlProvider using the generated bindings
impl query::Handler<Option<Context>> for LibsqlProvider {
    #[instrument(level = "debug", skip_all, fields(query))]
    async fn query(
        &self,
        ctx: Option<Context>,
        query: String,
        params: Vec<LibsqlValue>,
    ) -> Result<Result<Vec<ResultRow>, QueryError>> {
        propagate_trace_for_ctx!(ctx);
        let Some(Context {
            component: Some(source_id),
            ..
        }) = ctx
        else {
            return Ok(Err(QueryError::Unexpected(
                "unexpectedly missing source ID".into(),
            )));
        };

        Ok(self.do_query(&source_id, &query, params).await)
    }
}

async fn batch_rows_to_result_rows(mut batch_rows: libsql::BatchRows) -> Vec<ResultRow> {
    let mut result_rows = Vec::new();
    while let Some(Some(rows)) = batch_rows.next_stmt_row() {
        result_rows.extend(rows_to_result_rows(rows).await);
    }
    result_rows
}

async fn rows_to_result_rows(mut rows: libsql::Rows) -> Vec<ResultRow> {
    let mut result_rows = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        result_rows.push(row_to_result_row(&row));
    }
    result_rows
}

fn row_to_result_row(row: &libsql::Row) -> ResultRow {
    let mut result_row = ResultRow::default();
    let column_count = row.column_count();
    for i in 0..column_count {
        let name = row.column_name(i).unwrap_or_default().to_string();
        let value = LibsqlValue::from(row.get_value(i).unwrap());
        let entry = ResultRowEntry {
            column_name: name,
            value,
        };
        result_row.push(entry);
    }
    result_row
}
