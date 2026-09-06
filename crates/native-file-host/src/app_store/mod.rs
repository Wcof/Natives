//! App Store registry data layer (ADR-0025 D13/D14) — Phase A2.
//!
//! Authoritative Apps Domain tables inside the shared `natives.db`:
//! `apps`, `app_packages`, `app_permissions`, `app_install_transactions`
//! plus the `app_meta` projection-revision row. Capability-based
//! migrations (`CREATE TABLE IF NOT EXISTS` + `table_has_column`); this
//! store must NOT touch the shared `PRAGMA user_version` (ADR-0025 D14).
//!
//! Submodules:
//! - `types`: App / AppPackage / AppPermission / InstallTransaction + `AppError`
//! - `schema`: DDL + capability migrations
//! - `query`: read-side queries
//!
//! The `apps:*` Native Messaging dispatch and the mutation layer land with
//! Phase A5 package handling, so this module is declared without a caller
//! yet; dead code is allowed explicitly for that staged window.
#![allow(dead_code)]

pub mod query;
pub mod schema;
pub mod types;
