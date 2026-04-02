use rusqlite::Connection;

pub fn insert_session(
    conn: &Connection,
    task_id: i64,
    task_title: &str,
    duration_seconds: i64,
    completed: bool,
    started_at: &str,
    ended_at: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO focus_sessions (task_id, task_title, duration_seconds, completed, started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![task_id, task_title, duration_seconds, completed as i32, started_at, ended_at],
    )?;
    Ok(())
}
