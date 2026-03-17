use chrono::{Datelike, Local, NaiveDate};
use color_eyre::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: i64,
    pub title: String,
    pub week_start: NaiveDate,
    pub progress: u8, // 0-100
    pub completed: bool,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Returns the Monday of the week containing `date`.
pub fn week_start(date: NaiveDate) -> NaiveDate {
    let days_from_monday = date.weekday().num_days_from_monday();
    date - chrono::Duration::days(days_from_monday as i64)
}

pub fn list_for_week(conn: &Connection, week: NaiveDate) -> Result<Vec<Goal>> {
    let ws = week_start(week).format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT id, title, week_start, progress, completed, notes, created_at, updated_at
         FROM goals WHERE week_start = ?1 ORDER BY id ASC",
    )?;
    let goals = stmt
        .query_map(params![ws], |row| {
            Ok(Goal {
                id: row.get(0)?,
                title: row.get(1)?,
                week_start: {
                    let s: String = row.get(2)?;
                    NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                        .unwrap_or_else(|_| Local::now().date_naive())
                },
                progress: row.get::<_, i64>(3)? as u8,
                completed: row.get::<_, i64>(4)? != 0,
                notes: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(goals)
}

pub fn insert(conn: &Connection, title: &str, week: NaiveDate) -> Result<Goal> {
    let now = Local::now().to_rfc3339();
    let ws = week_start(week).format("%Y-%m-%d").to_string();
    conn.execute(
        "INSERT INTO goals (title, week_start, progress, completed, created_at, updated_at)
         VALUES (?1, ?2, 0, 0, ?3, ?3)",
        params![title, ws, now],
    )?;
    let id = conn.last_insert_rowid();
    Ok(Goal {
        id,
        title: title.to_string(),
        week_start: week_start(week),
        progress: 0,
        completed: false,
        notes: None,
        created_at: now.clone(),
        updated_at: now,
    })
}

pub fn set_progress(conn: &Connection, id: i64, progress: u8) -> Result<()> {
    let now = Local::now().to_rfc3339();
    let completed = if progress >= 100 { 1 } else { 0 };
    conn.execute(
        "UPDATE goals SET progress = ?1, completed = ?2, updated_at = ?3 WHERE id = ?4",
        params![progress, completed, now, id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM goals WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE goals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                week_start TEXT NOT NULL,
                progress INTEGER NOT NULL DEFAULT 0,
                completed INTEGER NOT NULL DEFAULT 0,
                notes TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_week_start_is_monday() {
        // 2026-03-17 is a Tuesday
        let tue = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let mon = week_start(tue);
        assert_eq!(mon.weekday(), Weekday::Mon);
        assert_eq!(mon, NaiveDate::from_ymd_opt(2026, 3, 16).unwrap());
    }

    #[test]
    fn test_insert_and_list() {
        let conn = setup();
        let week = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        insert(&conn, "Ship auth service", week).unwrap();
        let goals = list_for_week(&conn, week).unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].title, "Ship auth service");
        assert_eq!(goals[0].progress, 0);
    }

    #[test]
    fn test_set_progress_completes_at_100() {
        let conn = setup();
        let week = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let goal = insert(&conn, "Run 3x", week).unwrap();
        set_progress(&conn, goal.id, 100).unwrap();
        let goals = list_for_week(&conn, week).unwrap();
        assert!(goals[0].completed);
        assert_eq!(goals[0].progress, 100);
    }

    #[test]
    fn test_progress_below_100_not_completed() {
        let conn = setup();
        let week = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let goal = insert(&conn, "Run 3x", week).unwrap();
        set_progress(&conn, goal.id, 66).unwrap();
        let goals = list_for_week(&conn, week).unwrap();
        assert!(!goals[0].completed);
        assert_eq!(goals[0].progress, 66);
    }

    #[test]
    fn test_delete() {
        let conn = setup();
        let week = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let goal = insert(&conn, "Delete me", week).unwrap();
        delete(&conn, goal.id).unwrap();
        assert!(list_for_week(&conn, week).unwrap().is_empty());
    }
}
