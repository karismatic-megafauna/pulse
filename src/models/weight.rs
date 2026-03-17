use chrono::{Local, NaiveDate};
use color_eyre::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightEntry {
    pub id: i64,
    pub date: NaiveDate,
    pub weight: f64,
    pub notes: Option<String>,
    pub created_at: String,
}

pub fn get_for_date(conn: &Connection, date: NaiveDate) -> Result<Option<WeightEntry>> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT id, date, weight, notes, created_at
         FROM weight_logs WHERE date = ?1 ORDER BY id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![date_str], |row| {
        Ok(WeightEntry {
            id: row.get(0)?,
            date: {
                let s: String = row.get(1)?;
                NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                    .unwrap_or_else(|_| Local::now().date_naive())
            },
            weight: row.get(2)?,
            notes: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    Ok(rows.next().and_then(|r| r.ok()))
}

pub fn list_recent(conn: &Connection, days: u32) -> Result<Vec<WeightEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, weight, notes, created_at
         FROM weight_logs ORDER BY date ASC",
    )?;
    let all: Vec<WeightEntry> = stmt
        .query_map([], |row| {
            Ok(WeightEntry {
                id: row.get(0)?,
                date: {
                    let s: String = row.get(1)?;
                    NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                        .unwrap_or_else(|_| Local::now().date_naive())
                },
                weight: row.get(2)?,
                notes: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    let cutoff = Local::now().date_naive() - chrono::Duration::days(days as i64);
    Ok(all.into_iter().filter(|e| e.date >= cutoff).collect())
}

pub fn upsert(conn: &Connection, date: NaiveDate, weight: f64, notes: Option<&str>) -> Result<WeightEntry> {
    let now = Local::now().to_rfc3339();
    let date_str = date.format("%Y-%m-%d").to_string();

    // Delete existing entry for the day then insert fresh
    conn.execute("DELETE FROM weight_logs WHERE date = ?1", params![date_str])?;
    conn.execute(
        "INSERT INTO weight_logs (date, weight, notes, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![date_str, weight, notes, now],
    )?;
    Ok(WeightEntry {
        id: conn.last_insert_rowid(),
        date,
        weight,
        notes: notes.map(|s| s.to_string()),
        created_at: now,
    })
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM weight_logs WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE weight_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,
                weight REAL NOT NULL,
                notes TEXT,
                created_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_upsert_and_get() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        upsert(&conn, date, 175.5, None).unwrap();
        let entry = get_for_date(&conn, date).unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().weight, 175.5);
    }

    #[test]
    fn test_upsert_replaces_existing() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        upsert(&conn, date, 175.5, None).unwrap();
        upsert(&conn, date, 174.8, Some("morning")).unwrap();
        let entry = get_for_date(&conn, date).unwrap().unwrap();
        assert_eq!(entry.weight, 174.8);
    }

    #[test]
    fn test_list_recent() {
        let conn = setup();
        let today = Local::now().date_naive();
        let yesterday = today - chrono::Duration::days(1);
        let old = today - chrono::Duration::days(60);
        upsert(&conn, today, 175.0, None).unwrap();
        upsert(&conn, yesterday, 175.2, None).unwrap();
        upsert(&conn, old, 180.0, None).unwrap();

        let recent = list_recent(&conn, 30).unwrap();
        assert_eq!(recent.len(), 2);
    }
}
