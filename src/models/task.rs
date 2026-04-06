use chrono::{Local, NaiveDate};
use color_eyre::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub date: NaiveDate,
    pub completed: bool,
    pub priority: Priority,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Normal = 0,
    High = 1,
    Urgent = 2,
}

impl From<i64> for Priority {
    fn from(v: i64) -> Self {
        match v {
            1 => Priority::High,
            2 => Priority::Urgent,
            _ => Priority::Normal,
        }
    }
}

pub fn list_for_date(conn: &Connection, date: NaiveDate) -> Result<Vec<Task>> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT id, title, description, date, completed, priority, created_at, updated_at
         FROM tasks WHERE date = ?1 ORDER BY priority DESC, id ASC",
    )?;

    let tasks = stmt
        .query_map(params![date_str], |row| {
            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                date: {
                    let s: String = row.get(3)?;
                    NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                        .unwrap_or_else(|_| Local::now().date_naive())
                },
                completed: row.get::<_, i64>(4)? != 0,
                priority: Priority::from(row.get::<_, i64>(5)?),
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(tasks)
}

pub fn insert(conn: &Connection, title: &str, date: NaiveDate) -> Result<Task> {
    let now = Local::now().to_rfc3339();
    let date_str = date.format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO tasks (title, date, completed, priority, created_at, updated_at)
         VALUES (?1, ?2, 0, 0, ?3, ?3)",
        params![title, date_str, now],
    )?;

    let id = conn.last_insert_rowid();
    Ok(Task {
        id,
        title: title.to_string(),
        description: None,
        date,
        completed: false,
        priority: Priority::Normal,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Copy incomplete tasks from `from_date` to `to_date` (skips if task title already exists on to_date)
pub fn rollover_incomplete(conn: &Connection, from_date: NaiveDate, to_date: NaiveDate) -> Result<usize> {
    let from_tasks = list_for_date(conn, from_date)?;
    let to_tasks = list_for_date(conn, to_date)?;
    let existing_titles: std::collections::HashSet<&str> =
        to_tasks.iter().map(|t| t.title.as_str()).collect();

    let mut count = 0;
    for task in &from_tasks {
        if !task.completed && !existing_titles.contains(task.title.as_str()) {
            insert(conn, &task.title, to_date)?;
            count += 1;
        }
    }
    Ok(count)
}

pub fn toggle_complete(conn: &Connection, id: i64) -> Result<bool> {
    let now = Local::now().to_rfc3339();
    let current: i64 = conn.query_row(
        "SELECT completed FROM tasks WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    let new_val = if current == 0 { 1i64 } else { 0i64 };
    conn.execute(
        "UPDATE tasks SET completed = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_val, now, id],
    )?;
    Ok(new_val != 0)
}

pub fn update_title(conn: &Connection, id: i64, title: &str) -> Result<()> {
    let now = Local::now().to_rfc3339();
    conn.execute(
        "UPDATE tasks SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now, id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    Ok(())
}

#[allow(dead_code)]
pub fn count_for_date(conn: &Connection, date: NaiveDate) -> Result<(usize, usize)> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let total: usize = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE date = ?1",
        params![date_str],
        |row| row.get(0),
    )?;
    let done: usize = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE date = ?1 AND completed = 1",
        params![date_str],
        |row| row.get(0),
    )?;
    Ok((done, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        // run migrations inline for test isolation
        conn.execute_batch(
            "CREATE TABLE tasks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                description TEXT,
                date TEXT NOT NULL,
                completed INTEGER NOT NULL DEFAULT 0,
                priority INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_insert_and_list() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        insert(&conn, "Test task", date).unwrap();
        let tasks = list_for_date(&conn, date).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Test task");
        assert!(!tasks[0].completed);
    }

    #[test]
    fn test_toggle_complete() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let task = insert(&conn, "Toggle me", date).unwrap();
        let now_done = toggle_complete(&conn, task.id).unwrap();
        assert!(now_done);
        let back = toggle_complete(&conn, task.id).unwrap();
        assert!(!back);
    }

    #[test]
    fn test_delete() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let task = insert(&conn, "Delete me", date).unwrap();
        delete(&conn, task.id).unwrap();
        let tasks = list_for_date(&conn, date).unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_count_for_date() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        insert(&conn, "Task 1", date).unwrap();
        let t2 = insert(&conn, "Task 2", date).unwrap();
        toggle_complete(&conn, t2.id).unwrap();

        let (done, total) = count_for_date(&conn, date).unwrap();
        assert_eq!(total, 2);
        assert_eq!(done, 1);
    }

    #[test]
    fn test_update_title() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let task = insert(&conn, "Old title", date).unwrap();
        update_title(&conn, task.id, "New title").unwrap();
        let tasks = list_for_date(&conn, date).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "New title");
    }

    #[test]
    fn test_different_dates_isolated() {
        let conn = setup();
        let d1 = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 3, 18).unwrap();
        insert(&conn, "Today task", d1).unwrap();
        insert(&conn, "Tomorrow task", d2).unwrap();

        let today_tasks = list_for_date(&conn, d1).unwrap();
        let tomorrow_tasks = list_for_date(&conn, d2).unwrap();
        assert_eq!(today_tasks.len(), 1);
        assert_eq!(tomorrow_tasks.len(), 1);
    }
}
