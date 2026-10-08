//! Projects (FR-64): named, coloured groups of sessions. The list lives in `projects.json` in the
//! output root, beside the session folders, so it travels with them; each session stores only its
//! project's ID (`project_set` in its log).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::model::Session;
use super::recovery::write_atomic;

pub const PROJECTS_FILE: &str = "projects.json";

/// The colours a project can have, by name; the UI draws each in a light and a dark shade. No
/// red: red is the voice you hear.
pub const COLORS: [&str; 7] = ["slate", "blue", "teal", "green", "amber", "violet", "brown"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectsFile {
    v: u32,
    projects: Vec<Project>,
}

/// The projects under `root`; none when the file is missing or unreadable.
pub fn load(root: &Path) -> Vec<Project> {
    let path = root.join(PROJECTS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    match serde_json::from_str::<ProjectsFile>(&text) {
        Ok(f) => f.projects,
        Err(e) => {
            tracing::warn!("{}: {e}", path.display());
            Vec::new()
        }
    }
}

fn save(root: &Path, projects: &[Project]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let file = ProjectsFile { v: 1, projects: projects.to_vec() };
    let json = serde_json::to_string_pretty(&file).expect("projects always serialize");
    write_atomic(&root.join(PROJECTS_FILE), json.as_bytes())
}

/// A name trimmed to one line of at most 40 characters; `None` when nothing is left.
fn clean_name(name: &str) -> Option<String> {
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect();
    (!name.is_empty()).then_some(name)
}

fn clean_color(color: &str) -> String {
    if COLORS.contains(&color) { color.to_string() } else { COLORS[0].to_string() }
}

pub fn create(root: &Path, name: &str, color: &str) -> std::io::Result<Project> {
    let name = clean_name(name).ok_or_else(|| std::io::Error::other("empty project name"))?;
    let mut projects = load(root);
    let project = Project { id: ulid::Ulid::generate().to_string(), name, color: clean_color(color) };
    projects.push(project.clone());
    save(root, &projects)?;
    Ok(project)
}

/// Renames or recolours a project; an empty name keeps the old one.
pub fn update(root: &Path, changed: &Project) -> std::io::Result<Vec<Project>> {
    let mut projects = load(root);
    let p = projects.iter_mut().find(|p| p.id == changed.id).ok_or_else(|| std::io::Error::other("no such project"))?;
    if let Some(name) = clean_name(&changed.name) {
        p.name = name;
    }
    p.color = clean_color(&changed.color);
    save(root, &projects)?;
    Ok(projects)
}

/// Deletes a project. Its sessions keep their ID, which no longer names a project, so they show
/// as in none; nothing in their folders is touched.
pub fn delete(root: &Path, id: &str) -> std::io::Result<Vec<Project>> {
    let mut projects = load(root);
    projects.retain(|p| p.id != id);
    save(root, &projects)?;
    Ok(projects)
}

/// The name of a session's project, looked up beside its folder (for the Markdown front matter).
pub fn name_for(folder: &Path, session: &Session) -> Option<String> {
    let id = session.project.as_deref()?;
    let root = folder.parent()?;
    load(root).into_iter().find(|p| p.id == id).map(|p| p.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_round_trip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_empty());
        let a = create(dir.path(), "  ゼミ   第3回 ", "teal").unwrap();
        assert_eq!((a.name.as_str(), a.color.as_str()), ("ゼミ 第3回", "teal"));
        let b = create(dir.path(), "Release", "red").unwrap();
        // Red is not a project colour.
        assert_eq!(b.color, "slate");
        assert!(create(dir.path(), "   ", "blue").is_err());
        let list = update(dir.path(), &Project { id: b.id.clone(), name: "".into(), color: "blue".into() }).unwrap();
        assert_eq!(list[1], Project { id: b.id.clone(), name: "Release".into(), color: "blue".into() });
        let list = delete(dir.path(), &a.id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(load(dir.path()), list);
    }

    #[test]
    fn a_broken_file_reads_as_no_projects() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(PROJECTS_FILE), "{ not json").unwrap();
        assert!(load(dir.path()).is_empty());
    }
}
