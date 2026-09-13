//! App Store install transaction abort, rollback, and recovery operations.

use super::mutation::{lock_error, now_millis, AppStore};
use super::types::{install_state, AppError, InstallTransaction};
use crate::{app_host, app_install};
use rusqlite::params;

impl AppStore {
    pub fn install_abort(
        &self,
        install_id: &str,
        code: &str,
        message: &str,
    ) -> Result<InstallTransaction, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let record = self.transaction(install_id)?;
        if record.state == install_state::FAILED {
            return Ok(record);
        }
        let record = self.owned_transaction(install_id)?;
        self.abort_owned(&record, code, message)?;
        self.transaction(install_id)
    }

    pub(super) fn abort_owned(
        &self,
        record: &InstallTransaction,
        code: &str,
        message: &str,
    ) -> Result<(), AppError> {
        // Release the install lock FIRST: a rollback failure must not leak
        // the lock in self.installs (flock self-conflict on the next begin).
        // The removed File drops immediately (temporary value).
        self.installs
            .lock()
            .map_err(lock_error)?
            .remove(&record.install_id);
        self.rollback_install(record, code, message)
    }

    pub(super) fn rollback_install(
        &self,
        record: &InstallTransaction,
        code: &str,
        message: &str,
    ) -> Result<(), AppError> {
        let journal = self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT rollback_json FROM app_install_transactions WHERE install_id = ?1",
                [&record.install_id],
                |row| row.get::<_, String>(0),
            )?)
        })?;
        let phase = self.transaction(&record.install_id)?.state;
        if !journal.is_empty() {
            let _ = phase;
            app_host::remove_version(self.app_root(), &record.app_id, &record.to_version)?;
        }
        let staging =
            app_install::staging_dir(self.app_root(), &record.app_id, &record.install_id)?;
        app_install::remove_staging_dir(&staging)?;
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM app_package_stages WHERE install_id = ?1",
                [&record.install_id],
            )?;
            tx.execute(
                "UPDATE app_install_transactions SET state = 'failed', rollback_json = '', completed_at = ?2, error_code = ?3, error_message = ?4 WHERE install_id = ?1",
                params![
                    record.install_id,
                    now_millis(),
                    code.chars().take(64).collect::<String>(),
                    message.chars().take(256).collect::<String>()
                ],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub(super) fn finish_committed(&self, record: &InstallTransaction) -> Result<(), AppError> {
        let staging =
            app_install::staging_dir(self.app_root(), &record.app_id, &record.install_id)?;
        app_install::remove_staging_dir(&staging)?;
        // Contract §5 step 7: keep the new version plus ONE previous version;
        // anything older is cleaned. The update itself does not run the app.
        if !record.from_version.is_empty() && record.from_version != record.to_version {
            app_host::retain_versions(
                self.app_root(),
                &record.app_id,
                &[record.to_version.as_str(), record.from_version.as_str()],
            )?;
        } else {
            app_host::retain_versions(
                self.app_root(),
                &record.app_id,
                &[record.to_version.as_str()],
            )?;
        }
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM app_package_stages WHERE install_id = ?1",
                [&record.install_id],
            )?;
            tx.execute(
                "UPDATE app_install_transactions SET rollback_json = '' WHERE install_id = ?1",
                [&record.install_id],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub(super) fn recover_app(&self, app_id: &str) -> Result<(), AppError> {
        let ids = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT install_id FROM app_install_transactions WHERE app_id = ?1 AND (state NOT IN ('installed', 'failed') OR rollback_json != '') ORDER BY started_at",
            )?;
            let rows = stmt.query_map([app_id], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        for id in ids {
            let record = self.transaction(&id)?;
            if record.state == install_state::INSTALLED {
                self.finish_committed(&record)?;
            } else {
                self.rollback_install(&record, "APP_INTERRUPTED", "interrupted install recovered")?;
            }
        }
        Ok(())
    }

    /// Roll back this connection's own leftover transactions for `app_id`
    /// (ones still registered in `self.installs`, i.e. holding the install
    /// lock). Called BEFORE acquiring the app lock in the begin methods so
    /// a same-connection retry after a failed install cannot self-conflict
    /// on the flock file. Transactions owned by another connection (not in
    /// `self.installs`) are left untouched — `recover_app` after the lock
    /// acquisition owns that decision.
    pub(super) fn recover_owned_stale(&self, app_id: &str) -> Result<(), AppError> {
        let ids = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT install_id FROM app_install_transactions WHERE app_id = ?1 AND (state NOT IN ('installed', 'failed') OR rollback_json != '') ORDER BY started_at",
            )?;
            let rows = stmt.query_map([app_id], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let mut owned = self.installs.lock().map_err(lock_error)?;
        for id in ids {
            // Only roll back what THIS connection still owns (lock held in
            // the map). Removing the entry drops the install lock.
            if owned.remove(&id).is_some() {
                let record = self.transaction(&id)?;
                if record.state == install_state::INSTALLED {
                    drop(owned);
                    self.finish_committed(&record)?;
                    owned = self.installs.lock().map_err(lock_error)?;
                } else {
                    drop(owned);
                    self.rollback_install(
                        &record,
                        "APP_INTERRUPTED",
                        "interrupted install recovered",
                    )?;
                    owned = self.installs.lock().map_err(lock_error)?;
                }
            }
        }
        Ok(())
    }

    pub fn recover_interrupted(&self) -> Result<(), AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let ids = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT app_id FROM app_install_transactions WHERE state NOT IN ('installed','failed') OR rollback_json != ''",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        for id in ids {
            match app_install::acquire_app_lock(self.app_root(), &id, false) {
                Ok(_lock) => self.recover_app(&id)?,
                Err(AppError::Conflict(_)) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub fn recover_install(&self, app_id: &str) -> Result<(), AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let _install = app_install::acquire_app_lock(self.app_root(), app_id, false)?;
        self.recover_app(app_id)
    }
}
