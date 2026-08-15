//! GitHub API 极小封装（订阅引擎专用）。
//!
//! 限速策略：REST（api.github.com）匿名 60 次/小时——同步算法只花 2 次/轮
//! （分支 head + trees recursive），正文走 raw.githubusercontent.com CDN（不计 REST）。
//! 可选 token（env `GITHUB_TOKEN` 优先于 config）提升到 5000 次/小时。

use serde::Deserialize;

const GITHUB_API: &str = "https://api.github.com";
const GITHUB_RAW: &str = "https://raw.githubusercontent.com";
const USER_AGENT: &str = "gaubeeos-server";

#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    #[error("网络: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("仓库不存在或不可访问: {owner}/{repo}")]
    RepoNotFound { owner: String, repo: String },
    #[error("ref 不存在: {git_ref}")]
    RefNotFound { git_ref: String },
    #[error("API {status}: {body}")]
    Api { status: u16, body: String },
    #[error("树被截断（仓库过大）：请收窄 include 的路径范围")]
    TreeTruncated,
}

pub struct GitHubClient {
    http: reqwest::Client,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RepoInfo {
    default_branch: String,
}

#[derive(Debug, Deserialize)]
struct CommitRef {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    sha: String,
}

#[derive(Debug, Deserialize)]
struct TreeResponse {
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    tree: Vec<TreeEntry>,
}

/// trees API 的一个 blob 条目（已过滤目录）。
#[derive(Debug, Clone)]
pub struct BlobRef {
    pub path: String,
    pub sha: String,
}

impl GitHubClient {
    pub fn new(token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .gzip(true)
                .build()
                .expect("reqwest client"),
            token,
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{GITHUB_API}{path}")
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, GitHubError> {
        let mut req = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT);
        if let Some(tok) = &self.token {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            // 401/403 语义细分交给上层（token 失效 vs 限速），这里统一带状态码返回
            return Err(GitHubError::Api {
                status: status.as_u16(),
                body: body.chars().take(200).collect(),
            });
        }
        Ok(resp.json().await?)
    }

    /// 解析 ref 的 head commit sha。
    /// - 空 ref → 仓库默认分支（2 次 REST：repo info + commits/{branch}）
    /// - 40-hex sha → 原样返回（0 次 REST）
    /// - 分支/tag 名 → commits/{ref}（1 次 REST）
    ///
    /// 返回 (commit_sha, 解析出的 ref 名)——ref 名用于 raw URL 与状态展示。
    pub async fn resolve_head(
        &self,
        owner: &str,
        repo: &str,
        git_ref: &str,
    ) -> Result<(String, String), GitHubError> {
        let is_sha = git_ref.len() == 40 && git_ref.chars().all(|c| c.is_ascii_hexdigit());
        if is_sha {
            return Ok((git_ref.to_string(), git_ref.to_string()));
        }
        let branch = if git_ref.is_empty() {
            self.default_branch(owner, repo).await?
        } else {
            git_ref.to_string()
        };
        let c: CommitRef = self
            .get_json(&self.api_url(&format!("/repos/{owner}/{repo}/commits/{branch}")))
            .await
            .map_err(|e| match e {
                GitHubError::Api { status: 404, .. } | GitHubError::Api { status: 422, .. } => {
                    GitHubError::RefNotFound {
                        git_ref: branch.clone(),
                    }
                }
                other => other,
            })?;
        Ok((c.sha, branch))
    }

    /// 默认分支名（供 state 缓存与 raw URL）。
    pub async fn default_branch(&self, owner: &str, repo: &str) -> Result<String, GitHubError> {
        let info: RepoInfo = self
            .get_json(&self.api_url(&format!("/repos/{owner}/{repo}")))
            .await
            .map_err(|e| match e {
                GitHubError::Api { status: 404, .. } => GitHubError::RepoNotFound {
                    owner: owner.into(),
                    repo: repo.into(),
                },
                other => other,
            })?;
        Ok(info.default_branch)
    }

    /// 拉取某 commit 的整棵文件树（blob 条目）。
    pub async fn fetch_tree(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Result<Vec<BlobRef>, GitHubError> {
        let tree: TreeResponse = self
            .get_json(&self.api_url(&format!(
                "/repos/{owner}/{repo}/git/trees/{sha}?recursive=1"
            )))
            .await?;
        if tree.truncated {
            return Err(GitHubError::TreeTruncated);
        }
        Ok(tree
            .tree
            .into_iter()
            .filter(|e| e.kind == "blob")
            .map(|e| BlobRef {
                path: e.path,
                sha: e.sha,
            })
            .collect())
    }

    /// raw CDN 下载文件正文（不计 REST 限额）。
    pub async fn fetch_raw(
        &self,
        owner: &str,
        repo: &str,
        commit_sha: &str,
        path: &str,
    ) -> Result<String, GitHubError> {
        let url = format!("{GITHUB_RAW}/{owner}/{repo}/{commit_sha}/{path}");
        let resp = self
            .http
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(GitHubError::Api {
                status: resp.status().as_u16(),
                body: path.into(),
            });
        }
        Ok(resp.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_constructs_without_token() {
        let c = GitHubClient::new(None);
        assert!(c.token.is_none());
    }
}
