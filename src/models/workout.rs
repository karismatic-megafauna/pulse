use chrono::{Local, NaiveDate};
use color_eyre::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workout {
    pub id: i64,
    pub date: NaiveDate,
    pub workout_type: String,
    pub duration_minutes: Option<u32>,
    pub notes: Option<String>,
    pub created_at: String,
}

pub fn list_for_date(conn: &Connection, date: NaiveDate) -> Result<Vec<Workout>> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT id, date, workout_type, duration_minutes, notes, created_at
         FROM workouts WHERE date = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map(params![date_str], |row| {
            Ok(Workout {
                id: row.get(0)?,
                date: {
                    let s: String = row.get(1)?;
                    NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                        .unwrap_or_else(|_| Local::now().date_naive())
                },
                workout_type: row.get(2)?,
                duration_minutes: row.get::<_, Option<i64>>(3)?.map(|v| v as u32),
                notes: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn list_for_range(
    conn: &Connection,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<Workout>> {
    let from_s = from.format("%Y-%m-%d").to_string();
    let to_s = to.format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT id, date, workout_type, duration_minutes, notes, created_at
         FROM workouts WHERE date >= ?1 AND date <= ?2 ORDER BY date DESC, id DESC",
    )?;
    let rows = stmt
        .query_map(params![from_s, to_s], |row| {
            Ok(Workout {
                id: row.get(0)?,
                date: {
                    let s: String = row.get(1)?;
                    NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                        .unwrap_or_else(|_| Local::now().date_naive())
                },
                workout_type: row.get(2)?,
                duration_minutes: row.get::<_, Option<i64>>(3)?.map(|v| v as u32),
                notes: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn insert(
    conn: &Connection,
    date: NaiveDate,
    workout_type: &str,
    duration_minutes: Option<u32>,
    notes: Option<&str>,
) -> Result<Workout> {
    let now = Local::now().to_rfc3339();
    let date_str = date.format("%Y-%m-%d").to_string();
    conn.execute(
        "INSERT INTO workouts (date, workout_type, duration_minutes, notes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            date_str,
            workout_type,
            duration_minutes.map(|d| d as i64),
            notes,
            now
        ],
    )?;
    Ok(Workout {
        id: conn.last_insert_rowid(),
        date,
        workout_type: workout_type.to_string(),
        duration_minutes,
        notes: notes.map(|s| s.to_string()),
        created_at: now,
    })
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM workouts WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE workouts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,
                workout_type TEXT NOT NULL,
                duration_minutes INTEGER,
                notes TEXT,
                created_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_insert_and_list() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        insert(&conn, date, "Strength", Some(45), Some("Upper body")).unwrap();
        let workouts = list_for_date(&conn, date).unwrap();
        assert_eq!(workouts.len(), 1);
        assert_eq!(workouts[0].workout_type, "Strength");
        assert_eq!(workouts[0].duration_minutes, Some(45));
    }

    #[test]
    fn test_list_for_range() {
        let conn = setup();
        let d1 = NaiveDate::from_ymd_opt(2026, 3, 16).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let d3 = NaiveDate::from_ymd_opt(2026, 3, 18).unwrap();
        insert(&conn, d1, "Running", Some(30), None).unwrap();
        insert(&conn, d2, "Cycling", Some(60), None).unwrap();
        insert(&conn, d3, "Yoga", None, None).unwrap();

        let range = list_for_range(&conn, d1, d2).unwrap();
        assert_eq!(range.len(), 2);
    }
}
