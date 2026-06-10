use super::*;

impl SqliteTenantStore {
    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        self.read_snapshot()?
            .scheduled_execution_exists(execution_id)
    }

    pub fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        self.execute_write(move |transaction| transaction.insert_scheduled_job(job))?;
        Ok(())
    }

    pub fn claim_due_jobs(&self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(now, max_jobs))?
            .value)
    }

    pub fn complete_scheduled_job(&self, job_id: &JobId) -> Result<()> {
        self.execute_write(move |transaction| transaction.complete_scheduled_job(job_id))?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&self, job_id: &JobId) -> Result<bool> {
        Ok(self
            .execute_write(move |transaction| transaction.cancel_scheduled_job(job_id))?
            .value)
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        self.read_snapshot()?.list_scheduled_jobs()
    }

    pub fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        self.execute_write(move |transaction| transaction.record_scheduled_job_result(result))?;
        Ok(())
    }

    pub fn get_scheduled_job_result(&self, job_id: &JobId) -> Result<Option<ScheduledJobResult>> {
        self.read_snapshot()?.get_scheduled_job_result(job_id)
    }

    pub fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        self.execute_write(move |transaction| transaction.save_cron_job(cron))?;
        Ok(())
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        self.read_snapshot()?.load_cron_jobs()
    }

    pub fn delete_cron_job(&self, name: &str) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_cron_job(name))?;
        Ok(())
    }

    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        self.read_snapshot()?.next_scheduled_work_at()
    }

    pub fn has_scheduled_work(&self) -> Result<bool> {
        self.read_snapshot()?.has_scheduled_work()
    }

    pub fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        self.execute_write(move |transaction| transaction.recover_running_jobs(now))?;
        Ok(())
    }
}

pub(super) fn begin_scheduled_execution_in_conn(
    conn: &Connection,
    execution_id: Option<&str>,
) -> Result<bool> {
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
            params![execution_id],
        )
        .map_err(map_sqlite_error)?;
    Ok(inserted == 1)
}

pub(super) fn load_scheduled_jobs_from_conn(
    conn: &Connection,
    table_name: &str,
) -> Result<Vec<ScheduledJob>> {
    let order_by = if table_name == "scheduled_jobs" {
        "run_at, id"
    } else {
        "id"
    };
    let sql = format!("SELECT data_json FROM {table_name} ORDER BY {order_by}");
    let mut stmt = conn.prepare(sql.as_str()).map_err(map_sqlite_error)?;
    let mut rows = stmt.query([]).map_err(map_sqlite_error)?;
    let mut jobs = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite_error)? {
        jobs.push(deserialize_json::<ScheduledJob>(
            row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str(),
        )?);
    }
    Ok(jobs)
}

pub(crate) fn scheduled_run_at_key(timestamp: Timestamp) -> String {
    format!("{:020}", timestamp.0)
}

pub(super) fn load_due_scheduled_jobs_from_conn(
    conn: &Connection,
    now: Timestamp,
    max_jobs: usize,
) -> Result<Vec<ScheduledJob>> {
    if max_jobs == 0 {
        return Ok(Vec::new());
    }
    let max_jobs = i64::try_from(max_jobs).unwrap_or(i64::MAX);
    let run_at_upper = scheduled_run_at_key(now);
    let mut stmt = conn
        .prepare(
            "SELECT data_json FROM scheduled_jobs
             WHERE run_at <= ?1
             ORDER BY run_at, id
             LIMIT ?2",
        )
        .map_err(map_sqlite_error)?;
    let mut rows = stmt
        .query(params![run_at_upper, max_jobs])
        .map_err(map_sqlite_error)?;
    let mut jobs = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite_error)? {
        jobs.push(deserialize_json::<ScheduledJob>(
            row.get::<_, String>(0).map_err(map_sqlite_error)?.as_str(),
        )?);
    }
    Ok(jobs)
}

pub(super) fn apply_schedule_ops_in_transaction(
    transaction: &mut SqliteWriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for schedule_op in schedule_ops {
        match schedule_op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                if !transaction.cancel_scheduled_job(job_id)? {
                    return Err(Error::ScheduledJobNotFound(job_id.clone()));
                }
            }
        }
    }
    Ok(())
}
