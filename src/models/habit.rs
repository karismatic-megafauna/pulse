use chrono::{Datelike, Duration, Local, NaiveDate};
use color_eyre::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Habit {
    pub id: i64,
    pub title: String,
    pub frequency: u8,
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct HabitWithProgress {
    pub habit: Habit,
    pub checkins_this_week: u8,
    pub completed: bool,
    pub streak: u32,
    pub checked_in_today: bool,
}

pub fn week_start(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

pub fn list_active(conn: &Connection) -> Result<Vec<Habit>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, frequency, active, created_at FROM habits WHERE active = 1 ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Habit {
                id: row.get(0)?,
                title: row.get(1)?,
                frequency: row.get::<_, u8>(2)?,
                active: row.get::<_, bool>(3)?,
                created_at: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn list_all(conn: &Connection) -> Result<Vec<Habit>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, frequency, active, created_at FROM habits ORDER BY active DESC, id",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Habit {
                id: row.get(0)?,
                title: row.get(1)?,
                frequency: row.get::<_, u8>(2)?,
                active: row.get::<_, bool>(3)?,
                created_at: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn insert(conn: &Connection, title: &str, frequency: u8) -> Result<Habit> {
    let now = Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO habits (title, frequency, active, created_at) VALUES (?1, ?2, 1, ?3)",
        params![title, frequency, now],
    )?;
    let id = conn.last_insert_rowid();
    Ok(Habit {
        id,
        title: title.to_string(),
        frequency,
        active: true,
        created_at: now,
    })
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM habits WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn toggle_active(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE habits SET active = CASE WHEN active = 1 THEN 0 ELSE 1 END WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn checkin(conn: &Connection, habit_id: i64, date: NaiveDate) -> Result<()> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let now = Local::now().to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO habit_checkins (habit_id, date, created_at) VALUES (?1, ?2, ?3)",
        params![habit_id, date_str, now],
    )?;
    Ok(())
}

pub fn uncheckin(conn: &Connection, habit_id: i64, date: NaiveDate) -> Result<()> {
    let date_str = date.format("%Y-%m-%d").to_string();
    conn.execute(
        "DELETE FROM habit_checkins WHERE habit_id = ?1 AND date = ?2",
        params![habit_id, date_str],
    )?;
    Ok(())
}

pub fn is_checked_in(conn: &Connection, habit_id: i64, date: NaiveDate) -> bool {
    let date_str = date.format("%Y-%m-%d").to_string();
    conn.query_row(
        "SELECT COUNT(*) FROM habit_checkins WHERE habit_id = ?1 AND date = ?2",
        params![habit_id, date_str],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

fn count_checkins_for_week(conn: &Connection, habit_id: i64, ws: NaiveDate) -> u8 {
    let start = ws.format("%Y-%m-%d").to_string();
    let end = (ws + Duration::days(6)).format("%Y-%m-%d").to_string();
    conn.query_row(
        "SELECT COUNT(*) FROM habit_checkins WHERE habit_id = ?1 AND date >= ?2 AND date <= ?3",
        params![habit_id, start, end],
        |row| row.get::<_, u8>(0),
    )
    .unwrap_or(0)
}

fn calculate_streak(conn: &Connection, habit_id: i64, frequency: u8, current_week_start: NaiveDate) -> u32 {
    let mut streak: u32 = 0;
    let mut ws = current_week_start - Duration::weeks(1);
    loop {
        let count = count_checkins_for_week(conn, habit_id, ws);
        if count >= frequency {
            streak += 1;
            ws = ws - Duration::weeks(1);
        } else {
            break;
        }
    }
    streak
}

pub fn list_with_progress(conn: &Connection, date: NaiveDate) -> Result<Vec<HabitWithProgress>> {
    build_progress(conn, date, list_active(conn)?)
}

pub fn list_all_with_progress(conn: &Connection, date: NaiveDate) -> Result<Vec<HabitWithProgress>> {
    build_progress(conn, date, list_all(conn)?)
}

fn build_progress(conn: &Connection, date: NaiveDate, habits: Vec<Habit>) -> Result<Vec<HabitWithProgress>> {
    let ws = week_start(date);
    let mut result = Vec::new();
    for habit in habits {
        let checkins = count_checkins_for_week(conn, habit.id, ws);
        let completed = checkins >= habit.frequency;
        let streak = calculate_streak(conn, habit.id, habit.frequency, ws);
        let checked_in_today = is_checked_in(conn, habit.id, date);
        result.push(HabitWithProgress {
            habit,
            checkins_this_week: checkins,
            completed,
            streak,
            checked_in_today,
        });
    }
    Ok(result)
}

pub fn toggle_checkin(conn: &Connection, habit_id: i64, date: NaiveDate) -> Result<()> {
    if is_checked_in(conn, habit_id, date) {
        uncheckin(conn, habit_id, date)
    } else {
        checkin(conn, habit_id, date)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute_batch(
            "CREATE TABLE habits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                frequency INTEGER NOT NULL DEFAULT 1,
                active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            );
            CREATE TABLE habit_checkins (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                habit_id INTEGER NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
                date TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(habit_id, date)
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_insert_and_list() {
        let conn = setup();
        insert(&conn, "Workout", 3).unwrap();
        insert(&conn, "Read", 5).unwrap();
        let habits = list_active(&conn).unwrap();
        assert_eq!(habits.len(), 2);
        assert_eq!(habits[0].title, "Workout");
        assert_eq!(habits[0].frequency, 3);
        assert_eq!(habits[1].title, "Read");
    }

    #[test]
    fn test_checkin_and_progress() {
        let conn = setup();
        let habit = insert(&conn, "Workout", 3).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 3, 23).unwrap(); // a Monday
        checkin(&conn, habit.id, monday).unwrap();
        checkin(&conn, habit.id, monday + Duration::days(2)).unwrap();

        let progress = list_with_progress(&conn, monday).unwrap();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].checkins_this_week, 2);
        assert!(!progress[0].completed);
    }

    #[test]
    fn test_checkin_completes_habit() {
        let conn = setup();
        let habit = insert(&conn, "Read", 2).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 3, 23).unwrap();
        checkin(&conn, habit.id, monday).unwrap();
        checkin(&conn, habit.id, monday + Duration::days(1)).unwrap();

        let progress = list_with_progress(&conn, monday).unwrap();
        assert!(progress[0].completed);
    }

    #[test]
    fn test_streak_calculation() {
        let conn = setup();
        let habit = insert(&conn, "Meditate", 1).unwrap();
        // Complete 3 consecutive weeks before current
        let current_monday = NaiveDate::from_ymd_opt(2026, 3, 23).unwrap();
        for w in 1..=3 {
            let day = current_monday - Duration::weeks(w);
            checkin(&conn, habit.id, day).unwrap();
        }
        let progress = list_with_progress(&conn, current_monday).unwrap();
        assert_eq!(progress[0].streak, 3);
    }

    #[test]
    fn test_toggle_checkin() {
        let conn = setup();
        let habit = insert(&conn, "Guitar", 2).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 3, 25).unwrap();

        assert!(!is_checked_in(&conn, habit.id, today));
        toggle_checkin(&conn, habit.id, today).unwrap();
        assert!(is_checked_in(&conn, habit.id, today));
        toggle_checkin(&conn, habit.id, today).unwrap();
        assert!(!is_checked_in(&conn, habit.id, today));
    }

    #[test]
    fn test_duplicate_checkin_ignored() {
        let conn = setup();
        let habit = insert(&conn, "Run", 1).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 3, 25).unwrap();
        checkin(&conn, habit.id, today).unwrap();
        checkin(&conn, habit.id, today).unwrap(); // should not error
        assert!(is_checked_in(&conn, habit.id, today));
    }

    #[test]
    fn test_delete_cascades() {
        let conn = setup();
        let habit = insert(&conn, "Swim", 1).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 3, 25).unwrap();
        checkin(&conn, habit.id, today).unwrap();
        delete(&conn, habit.id).unwrap();
        let habits = list_active(&conn).unwrap();
        assert!(habits.is_empty());
    }

    #[test]
    fn test_pause_and_resume() {
        let conn = setup();
        let habit = insert(&conn, "Yoga", 2).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 3, 25).unwrap();
        checkin(&conn, habit.id, today).unwrap();

        // Pause: should disappear from active list but remain in all list
        toggle_active(&conn, habit.id).unwrap();
        assert!(list_active(&conn).unwrap().is_empty());
        let all = list_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert!(!all[0].active);

        // list_with_progress excludes paused, list_all_with_progress includes it
        assert!(list_with_progress(&conn, today).unwrap().is_empty());
        let all_progress = list_all_with_progress(&conn, today).unwrap();
        assert_eq!(all_progress.len(), 1);
        assert_eq!(all_progress[0].checkins_this_week, 1);

        // Resume: should reappear in active list
        toggle_active(&conn, habit.id).unwrap();
        let active = list_active(&conn).unwrap();
        assert_eq!(active.len(), 1);
        assert!(active[0].active);
        // Check-in history preserved
        assert!(is_checked_in(&conn, habit.id, today));
    }
}
