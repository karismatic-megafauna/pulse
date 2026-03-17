use chrono::{Local, NaiveDate};
use color_eyre::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: i64,
    pub date: NaiveDate,
    pub content: String,
    pub mood: Option<u8>, // 1-5
    pub created_at: String,
    pub updated_at: String,
}

pub fn get_for_date(conn: &Connection, date: NaiveDate) -> Result<Option<JournalEntry>> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT id, date, content, mood, created_at, updated_at
         FROM journal_entries WHERE date = ?1 ORDER BY id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![date_str], |row| {
        Ok(JournalEntry {
            id: row.get(0)?,
            date: {
                let s: String = row.get(1)?;
                NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                    .unwrap_or_else(|_| Local::now().date_naive())
            },
            content: row.get(2)?,
            mood: row.get::<_, Option<i64>>(3)?.map(|v| v as u8),
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    Ok(rows.next().and_then(|r| r.ok()))
}

pub fn list_recent(conn: &Connection, limit: u32) -> Result<Vec<JournalEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, content, mood, created_at, updated_at
         FROM journal_entries ORDER BY date DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(JournalEntry {
                id: row.get(0)?,
                date: {
                    let s: String = row.get(1)?;
                    NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                        .unwrap_or_else(|_| Local::now().date_naive())
                },
                content: row.get(2)?,
                mood: row.get::<_, Option<i64>>(3)?.map(|v| v as u8),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Upsert: one entry per day — updates if exists, inserts if not.
pub fn upsert(
    conn: &Connection,
    date: NaiveDate,
    content: &str,
    mood: Option<u8>,
) -> Result<JournalEntry> {
    let now = Local::now().to_rfc3339();
    let date_str = date.format("%Y-%m-%d").to_string();

    let existing = get_for_date(conn, date)?;
    if let Some(entry) = existing {
        conn.execute(
            "UPDATE journal_entries SET content = ?1, mood = ?2, updated_at = ?3 WHERE id = ?4",
            params![content, mood.map(|m| m as i64), now, entry.id],
        )?;
        Ok(JournalEntry {
            id: entry.id,
            date,
            content: content.to_string(),
            mood,
            created_at: entry.created_at,
            updated_at: now,
        })
    } else {
        conn.execute(
            "INSERT INTO journal_entries (date, content, mood, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![date_str, content, mood.map(|m| m as i64), now],
        )?;
        Ok(JournalEntry {
            id: conn.last_insert_rowid(),
            date,
            content: content.to_string(),
            mood,
            created_at: now.clone(),
            updated_at: now,
        })
    }
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM journal_entries WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE journal_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,
                content TEXT NOT NULL,
                mood INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_upsert_insert() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        upsert(&conn, date, "Good day today", Some(4)).unwrap();
        let entry = get_for_date(&conn, date).unwrap().unwrap();
        assert_eq!(entry.content, "Good day today");
        assert_eq!(entry.mood, Some(4));
    }

    #[test]
    fn test_upsert_updates_existing() {
        let conn = setup();
        let date = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        upsert(&conn, date, "First draft", Some(3)).unwrap();
        upsert(&conn, date, "Updated entry", Some(5)).unwrap();
        let entries = list_recent(&conn, 10).unwrap();
        assert_eq!(entries.len(), 1); // only one per day
        assert_eq!(entries[0].content, "Updated entry");
        assert_eq!(entries[0].mood, Some(5));
    }

    #[test]
    fn test_list_recent_ordering() {
        let conn = setup();
        let d1 = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 3, 16).unwrap();
        let d3 = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        upsert(&conn, d1, "Day 1", None).unwrap();
        upsert(&conn, d2, "Day 2", None).unwrap();
        upsert(&conn, d3, "Day 3", None).unwrap();

        let entries = list_recent(&conn, 10).unwrap();
        // Should be most recent first
        assert_eq!(entries[0].date, d3);
        assert_eq!(entries[2].date, d1);
    }
}
