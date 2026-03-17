use reqwest::Client;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub title: String,
    pub source_branch: String,
    pub state: String,
    pub url: String,
    pub has_conflicts: bool,
    pub draft: bool,
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

pub async fn fetch(base_url: &str, private_token: &str) -> Result<Vec<MergeRequest>, String> {
    let url = format!(
        "{}/api/v4/merge_requests?state=opened&scope=assigned_to_me&per_page=15",
        base_url.trim_end_matches('/')
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(&url)
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
                state: mr["state"].as_str().unwrap_or("").to_string(),
                url: mr["web_url"].as_str().unwrap_or("").to_string(),
                has_conflicts: mr["has_conflicts"].as_bool().unwrap_or(false),
                draft: mr["draft"].as_bool().unwrap_or(false),
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
                "draft": false
            },
            {
                "title": "WIP: Refactor auth",
                "source_branch": "refactor/auth",
                "state": "opened",
                "web_url": "https://gitlab.com/foo/bar/-/merge_requests/43",
                "has_conflicts": true,
                "draft": true
            }
        ]"#).unwrap();

        let mrs: Vec<MergeRequest> = json
            .iter()
            .filter_map(|mr| {
                Some(MergeRequest {
                    title: mr["title"].as_str()?.to_string(),
                    source_branch: mr["source_branch"].as_str().unwrap_or("").to_string(),
                    state: mr["state"].as_str().unwrap_or("").to_string(),
                    url: mr["web_url"].as_str().unwrap_or("").to_string(),
                    has_conflicts: mr["has_conflicts"].as_bool().unwrap_or(false),
                    draft: mr["draft"].as_bool().unwrap_or(false),
                })
            })
            .collect();

        assert_eq!(mrs.len(), 2);
        assert_eq!(mrs[0].source_branch, "feature/login");
        assert!(!mrs[0].has_conflicts);
        assert!(mrs[1].draft);
        assert!(mrs[1].has_conflicts);
    }
}
