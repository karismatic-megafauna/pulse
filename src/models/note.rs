use color_eyre::Result;
use std::fs;
use std::path::PathBuf;

use crate::config::config_dir;

pub fn notes_dir() -> PathBuf {
    config_dir().join("notes")
}

pub fn ensure_notes_dir() -> Result<()> {
    let dir = notes_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct NoteMeta {
    pub filename: String,
    pub title: String,
    pub modified: std::time::SystemTime,
}

pub fn list_notes() -> Result<Vec<NoteMeta>> {
    ensure_notes_dir()?;
    let dir = notes_dir();
    let mut notes = Vec::new();

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let title = filename.trim_end_matches(".md").replace('-', " ");
            let modified = entry.metadata()?.modified().unwrap_or(std::time::UNIX_EPOCH);
            notes.push(NoteMeta {
                filename,
                title,
                modified,
            });
        }
    }

    // Most recently modified first
    notes.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(notes)
}

pub fn read_note(filename: &str) -> Result<String> {
    let path = notes_dir().join(filename);
    Ok(fs::read_to_string(&path)?)
}

pub fn note_path(filename: &str) -> PathBuf {
    notes_dir().join(filename)
}

pub fn create_note(name: &str) -> Result<String> {
    ensure_notes_dir()?;
    let filename = slugify(name);
    let path = notes_dir().join(&filename);
    if !path.exists() {
        fs::write(&path, format!("# {}\n\n", name))?;
    }
    Ok(filename)
}

pub fn delete_note(filename: &str) -> Result<()> {
    let path = notes_dir().join(filename);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_lowercase().next().unwrap_or(c)
            } else if c == ' ' {
                '-'
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.md", slug.trim_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("My Meeting Notes"), "my-meeting-notes.md");
        assert_eq!(slugify("Sprint 42 Retro"), "sprint-42-retro.md");
        assert_eq!(slugify("1:1 with boss"), "1_1-with-boss.md");
    }
}
