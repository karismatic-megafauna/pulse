use reqwest::Client;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MrKind {
    Mine,
    Review,
}

#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub title: String,
    pub source_branch: String,
    pub url: String,
    pub has_conflicts: bool,
    pub draft: bool,
    pub kind: MrKind,
    pub author: String,
}

#[derive(Debug, Clone)]
pub enum GitlabState {
    Idle,
    Loading,
    Ready(Vec<MergeRequest>),
    Error(String),
}

pub struct GitlabCache {
    pub state: GitlabState,
    last_fetched: Option<Instant>,
}

impl GitlabCache {
    pub fn new() -> Self {
        Self {
            state: GitlabState::Idle,
            last_fetched: None,
        }
    }

    pub fn needs_refresh(&self) -> bool {
        match &self.state {
            GitlabState::Loading => false,
            GitlabState::Idle | GitlabState::Error(_) => true,
            GitlabState::Ready(_) => self
                .last_fetched
                .map(|t| t.elapsed() >= CACHE_TTL)
                .unwrap_or(true),
        }
    }

    pub fn set_loading(&mut self) {
        self.state = GitlabState::Loading;
    }

    pub fn set_result(&mut self, result: Result<Vec<MergeRequest>, String>) {
        self.last_fetched = Some(Instant::now());
        self.state = match result {
            Ok(mrs) => GitlabState::Ready(mrs),
            Err(e) => GitlabState::Error(e),
        };
    }
}

pub async fn fetch(base_url: &str, private_token: &str, project: &str, ignore_authors: &[String]) -> Result<Vec<MergeRequest>, String> {
    let base = base_url.trim_end_matches('/');

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    // Build the MR endpoint prefix: project-scoped or global
    let mr_prefix = if project.is_empty() {
        format!("{}/api/v4/merge_requests", base)
    } else {
        let encoded_project = urlencoding::encode(project);
        format!("{}/api/v4/projects/{}/merge_requests", base, encoded_project)
    };

    // Fetch MRs authored by me (scope=created_by_me)
    let mine_url = format!(
        "{}?state=opened&scope=created_by_me&per_page=15",
        mr_prefix
    );
    let mine_future = fetch_mrs(&client, &mine_url, private_token, MrKind::Mine);

    // Fetch MRs where I'm a reviewer (need user ID first)
    let user_url = format!("{}/api/v4/user", base);
    let user_resp = client
        .get(&user_url)
        .header("PRIVATE-TOKEN", private_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("GitLab user request failed: {}", e))?;

    let user_id = if user_resp.status().is_success() {
        let user_json: serde_json::Value = user_resp
            .json()
            .await
            .map_err(|e| format!("GitLab user parse error: {}", e))?;
        user_json["id"].as_u64()
    } else {
        None
    };

    let mine = mine_future.await?;

    let reviews = if let Some(uid) = user_id {
        let review_url = format!(
            "{}?state=opened&reviewer_id={}&per_page=15",
            mr_prefix, uid
        );
        fetch_mrs(&client, &review_url, private_token, MrKind::Review).await?
    } else {
        vec![]
    };

    // Merge and deduplicate by URL (if an MR appears in both, keep as Mine)
    let mut seen = std::collections::HashSet::new();
    let mut merged = Vec::new();
    for mr in mine {
        if ignore_authors.iter().any(|a| a.eq_ignore_ascii_case(&mr.author)) {
            continue;
        }
        seen.insert(mr.url.clone());
        merged.push(mr);
    }
    for mr in reviews {
        if ignore_authors.iter().any(|a| a.eq_ignore_ascii_case(&mr.author)) {
            continue;
        }
        if seen.insert(mr.url.clone()) {
            merged.push(mr);
        }
    }

    Ok(merged)
}

async fn fetch_mrs(
    client: &Client,
    url: &str,
    private_token: &str,
    kind: MrKind,
) -> Result<Vec<MergeRequest>, String> {
    let response = client
        .get(url)
        .header("PRIVATE-TOKEN", private_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("GitLab request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("GitLab HTTP {}: {}", status, &body[..body.len().min(200)]));
    }

    let json: Vec<serde_json::Value> = response
        .json()
        .await
        .map_err(|e| format!("GitLab parse error: {}", e))?;

    let mrs = json
        .iter()
        .filter_map(|mr| {
            Some(MergeRequest {
                title: mr["title"].as_str()?.to_string(),
                source_branch: mr["source_branch"].as_str().unwrap_or("").to_string(),
                url: mr["web_url"].as_str().unwrap_or("").to_string(),
                has_conflicts: mr["has_conflicts"].as_bool().unwrap_or(false),
                draft: mr["draft"].as_bool().unwrap_or(false),
                kind,
                author: mr["author"]["name"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();

    Ok(mrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gitlab_mrs() {
        let json: Vec<serde_json::Value> = serde_json::from_str(r#"[
            {
                "title": "Add login page",
                "source_branch": "feature/login",
                "state": "opened",
                "web_url": "https://gitlab.com/foo/bar/-/merge_requests/42",
                "has_conflicts": false,
                "draft": false,
                "author": { "name": "Alice", "username": "alice" }
            },
            {
                "title": "WIP: Refactor auth",
                "source_branch": "refactor/auth",
                "state": "opened",
                "web_url": "https://gitlab.com/foo/bar/-/merge_requests/43",
                "has_conflicts": true,
                "draft": true,
                "author": { "name": "Bob", "username": "bob" }
            }
        ]"#).unwrap();

        let mrs: Vec<MergeRequest> = json
            .iter()
            .filter_map(|mr| {
                Some(MergeRequest {
                    title: mr["title"].as_str()?.to_string(),
                    source_branch: mr["source_branch"].as_str().unwrap_or("").to_string(),
                        url: mr["web_url"].as_str().unwrap_or("").to_string(),
                    has_conflicts: mr["has_conflicts"].as_bool().unwrap_or(false),
                    draft: mr["draft"].as_bool().unwrap_or(false),
                    kind: MrKind::Mine,
                    author: mr["author"]["name"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();

        assert_eq!(mrs.len(), 2);
        assert_eq!(mrs[0].source_branch, "feature/login");
        assert_eq!(mrs[0].author, "Alice");
        assert!(!mrs[0].has_conflicts);
        assert!(mrs[1].draft);
        assert!(mrs[1].has_conflicts);
    }
}
