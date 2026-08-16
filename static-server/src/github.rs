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

#[derive(Clone)]
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
#[derive(Debug, Deserialize)]
struct ContentsItem {
    name: String,
    sha: String,
    #[serde(rename = "type")]
    kind: String,
}

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
        self.get_json_with_token(url, None).await
    }

    /// 带「按请求 token」的 GET（会话验证用：用用户带来的 gh token 查身份，
    /// 不与会话外的 env token 混用）。
    async fn get_json_with_token<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
    ) -> Result<T, GitHubError> {
        let mut req = self
            .http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT);
        let bearer = token.or(self.token.as_deref());
        if let Some(tok) = bearer {
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

    // ---- Contents API（managerStore git 后端用）----

    /// 列目录下的 .json 文件 → (filename, blob sha)。目录不存在（404）→ 空列表。
    pub async fn list_contents_dir(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
    ) -> Result<Vec<(String, String)>, GitHubError> {
        let items: Vec<ContentsItem> = match self
            .get_json(&self.api_url(&format!("/repos/{owner}/{repo}/contents/{path}")))
            .await
        {
            Ok(v) => v,
            Err(GitHubError::Api { status: 404, .. }) => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        Ok(items
            .into_iter()
            .filter(|i| i.kind == "file" && i.name.ends_with(".json"))
            .map(|i| (i.name, i.sha))
            .collect())
    }

    /// 取文件 meta（sha，用于 PUT 乐观锁）。404 → Ok(None)。
    pub async fn get_contents_meta(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
    ) -> Result<Option<String>, GitHubError> {
        #[derive(Deserialize)]
        struct FileMeta {
            sha: String,
        }
        match self
            .get_json::<FileMeta>(&self.api_url(&format!("/repos/{owner}/{repo}/contents/{path}")))
            .await
        {
            Ok(m) => Ok(Some(m.sha)),
            Err(GitHubError::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// PUT 文件（新建带 sha=None，更新带 sha=Some）。需要 push 权限 token。
    pub async fn put_contents(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        message: &str,
        content_b64: &str,
        sha: Option<&str>,
    ) -> Result<(), GitHubError> {
        let mut body = serde_json::json!({ "message": message, "content": content_b64 });
        if let Some(s) = sha {
            body["sha"] = serde_json::json!(s);
        }
        let mut req = self
            .http
            .put(self.api_url(&format!("/repos/{owner}/{repo}/contents/{path}")))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/json")
            .body(body.to_string());
        if let Some(tok) = &self.token {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api {
                status: status.as_u16(),
                body: text.chars().take(200).collect(),
            });
        }
        Ok(())
    }

    /// DELETE 文件（带 sha）。
    pub async fn delete_contents(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        message: &str,
        sha: &str,
    ) -> Result<(), GitHubError> {
        let body = serde_json::json!({ "message": message, "sha": sha });
        let mut req = self
            .http
            .delete(self.api_url(&format!("/repos/{owner}/{repo}/contents/{path}")))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/json")
            .body(body.to_string());
        if let Some(tok) = &self.token {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(GitHubError::Api {
                status: status.as_u16(),
                body: text.chars().take(200).collect(),
            });
        }
        Ok(())
    }

    /// 用任意 GitHub token 验证身份（GET /user → login）。
    #[allow(clippy::result_large_err)]
    /// 会话交换的验证步骤：token 有效返回 login，无效（401/403）返回 Err。
    pub async fn fetch_user_login(&self, token: &str) -> Result<String, GitHubError> {
        #[derive(Deserialize)]
        struct User {
            login: String,
        }
        let u: User = self
            .get_json_with_token(&self.api_url("/user"), Some(token))
            .await
            .map_err(|e| match e {
                GitHubError::Api { status: 401, .. } | GitHubError::Api { status: 403, .. } => {
                    GitHubError::Api {
                        status: 401,
                        body: "GitHub token 无效或已过期".into(),
                    }
                }
                other => other,
            })?;
        Ok(u.login)
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
