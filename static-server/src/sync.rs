//! 订阅同步引擎：调度（每源定时循环）+ 同步算法（增量下载到缓存）。
//!
//! 同步算法（每轮每源，REST 消耗 ≤3 次）：
//! 1. resolve_head：空 ref → 默认分支（repo info + commits）；sha 直接用；分支名 1 次
//! 2. head sha == last_sha → Skipped（0 次后续调用）
//! 3. Trees recursive → glob 过滤 .md
//! 4. 逐文件对比 blob sha：变化才从 raw CDN 下载（不计 REST），未匹配的旧文件删除
//! 5. 更新 state（持久化 state/{id}.json）→ 重建全局 manifest
//!
//! 磁盘布局（{data_dir}）：
//!   config.toml                      订阅配置（事实源）
//!   state/{id}.json                  每源运行态（last sha/entries/file_shas/last_error）
//!   cache/sources/{id}/{仓库相对路径}  markdown 缓存
//!   cache/manifest.json              聚合清单（前端消费）

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use crate::config::{compile_glob, SourceConfig};
use crate::github::{BlobRef, GitHubClient, GitHubError};
use crate::manifest::{build_entry, rebuild_manifest, ManifestEntry};

/// 每源运行态（持久化）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SourceState {
    pub last_sha: Option<String>,
    /// 解析后的 ref（分支名），展示与 raw URL 用。
    pub resolved_ref: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    /// path → blob sha（增量：sha 未变不下载）。
    pub file_shas: HashMap<String, String>,
    pub entries: Vec<ManifestEntry>,
}

/// 内存运行时句柄（调度任务 + 互斥锁）。
pub struct SourceRuntime {
    pub task: JoinHandle<()>,
    /// 串行化同源的并发同步（定时 + 手动同时触发时）。
    pub lock: Arc<Mutex<()>>,
    pub state: SourceState,
}

/// 单轮同步结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncOutcome {
    pub skipped: bool,
    pub head_sha: String,
    pub downloaded: usize,
    pub removed: usize,
    pub total_files: usize,
    pub error: Option<String>,
}

/// 全局应用状态（axum State 注入）。
pub struct AppState {
    pub data_dir: PathBuf,
    pub gh: GitHubClient,
    pub config: RwLock<crate::config::AppConfig>,
    /// id → runtime（含 state；读写都过这个锁）。
    pub runtime: Mutex<HashMap<String, SourceRuntime>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf, token: Option<String>) -> Self {
        Self {
            data_dir,
            gh: GitHubClient::new(token),
            config: RwLock::new(crate::config::AppConfig::default()),
            runtime: Mutex::new(HashMap::new()),
        }
    }

    fn state_path(&self, id: &str) -> PathBuf {
        self.data_dir.join("state").join(format!("{id}.json"))
    }

    fn cache_dir(&self, id: &str) -> PathBuf {
        self.data_dir.join("cache").join("sources").join(id)
    }

    /// 启动时恢复：读 state 文件 + 生成清单（无任务句柄，稍后由 scheduler 补）。
    pub async fn restore_states(&self, ids: &[String]) {
        let mut runtime = self.runtime.lock().await;
        for id in ids {
            let state: SourceState = std::fs::read(self.state_path(id))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            runtime.insert(
                id.clone(),
                SourceRuntime {
                    task: tokio::spawn(std::future::pending()),
                    lock: Arc::new(Mutex::new(())),
                    state,
                },
            );
        }
        drop(runtime);
        let _ = self.rebuild_manifest().await;
    }

    /// 持久化单源 state。
    async fn persist_state(&self, id: &str, state: &SourceState) {
        let path = self.state_path(id);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(state) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    /// 聚合 runtime 内全部 entries → 写 manifest.json。
    pub async fn rebuild_manifest(&self) -> std::io::Result<()> {
        let runtime = self.runtime.lock().await;
        let per: HashMap<String, Vec<ManifestEntry>> = runtime
            .iter()
            .map(|(id, rt)| (id.clone(), rt.state.entries.clone()))
            .collect();
        drop(runtime);
        rebuild_manifest(&self.data_dir, &per)
    }
}

/// （重新）调度一个源：取消旧任务，保留已有 state，spawn 新的定时循环。
/// AppState 以 Arc 共享——调度循环持有克隆。
pub async fn schedule_source(state: &Arc<AppState>, cfg: SourceConfig) {
    let mut runtime = state.runtime.lock().await;
    let id = cfg.id.clone();
    let prev = runtime.remove(&id);
    let st = prev
        .map(|rt| {
            rt.task.abort();
            rt.state
        })
        .unwrap_or_default();
    let lock = Arc::new(Mutex::new(()));
    let task = tokio::spawn(schedule_loop(Arc::clone(state), cfg, lock.clone()));
    runtime.insert(
        id,
        SourceRuntime {
            task,
            lock,
            state: st,
        },
    );
}

/// 取消调度并移除运行时（DELETE 源时用；返回最终 state 供清理缓存）。
pub async fn unschedule_source(state: &Arc<AppState>, id: &str) -> Option<SourceState> {
    let mut runtime = state.runtime.lock().await;
    runtime.remove(id).map(|rt| {
        rt.task.abort();
        rt.state
    })
}

/// 停用一个源：终止调度任务但保留 runtime/state（内容仍可被清单聚合，
/// 停用语义是「停止同步」而非「内容消失」）。重新 enable 走 schedule_source。
pub async fn stop_source(state: &Arc<AppState>, id: &str) {
    let mut runtime = state.runtime.lock().await;
    if let Some(rt) = runtime.get_mut(id) {
        rt.task.abort();
        // 占位任务（永不完成）：复用 SourceRuntime 结构，保持 state 可聚合
        rt.task = tokio::spawn(std::future::pending());
    }
}

/// 调度循环：立即同步一次 → 按 interval 循环。
async fn schedule_loop(state: Arc<AppState>, cfg: SourceConfig, _lock: Arc<Mutex<()>>) {
    loop {
        let _ = sync_once(&state, &cfg).await;
        tokio::time::sleep(cfg.interval_duration()).await;
    }
}

/// 执行单轮同步（定时与手动共用；互斥防同源并发）。
pub async fn sync_once(state: &Arc<AppState>, cfg: &SourceConfig) -> SyncOutcome {
    // 取该源互斥锁（runtime 里注册过的），没有则临时建一个
    let lock = {
        let mut runtime = state.runtime.lock().await;
        let rt = runtime
            .entry(cfg.id.clone())
            .or_insert_with(|| SourceRuntime {
                task: tokio::spawn(std::future::pending()),
                lock: Arc::new(Mutex::new(())),
                state: SourceState::default(),
            });
        rt.lock.clone()
    };
    let _guard = lock.lock().await;

    let mut outcome = SyncOutcome {
        skipped: false,
        head_sha: String::new(),
        downloaded: 0,
        removed: 0,
        total_files: 0,
        error: None,
    };

    match sync_inner(state, cfg, &mut outcome).await {
        Ok(()) => {}
        Err(e) => outcome.error = Some(e.to_string()),
    }

    // 更新 state（last_sync_at / last_error）并持久化 + 重建清单
    let now = chrono::Utc::now().to_rfc3339();
    {
        let mut runtime = state.runtime.lock().await;
        if let Some(rt) = runtime.get_mut(&cfg.id) {
            rt.state.last_sync_at = Some(now.clone());
            rt.state.last_error = outcome.error.clone();
        }
        let snapshot = runtime.get(&cfg.id).map(|rt| rt.state.clone());
        if let Some(s) = snapshot {
            state.persist_state(&cfg.id, &s).await;
        }
    }
    let _ = state.rebuild_manifest().await;
    outcome
}

/// 同步主体（无锁版本，由 sync_once 持锁调用）。
async fn sync_inner(
    state: &Arc<AppState>,
    cfg: &SourceConfig,
    outcome: &mut SyncOutcome,
) -> Result<(), GitHubError> {
    let (head_sha, resolved_ref) = state
        .gh
        .resolve_head(&cfg.owner, &cfg.repo, &cfg.git_ref)
        .await?;
    outcome.head_sha = head_sha.clone();

    // 读当前 state
    let current: SourceState = {
        let runtime = state.runtime.lock().await;
        runtime
            .get(&cfg.id)
            .map(|rt| rt.state.clone())
            .unwrap_or_default()
    };

    // sha 未变 → 跳过（仍刷新 resolved_ref）
    if current.last_sha.as_deref() == Some(head_sha.as_str()) {
        outcome.skipped = true;
        outcome.total_files = current.entries.len();
        let mut runtime = state.runtime.lock().await;
        if let Some(rt) = runtime.get_mut(&cfg.id) {
            rt.state.resolved_ref = Some(resolved_ref);
        }
        return Ok(());
    }

    // 拉树 + glob 过滤
    let tree = state
        .gh
        .fetch_tree(&cfg.owner, &cfg.repo, &head_sha)
        .await?;
    let include = compile_glob(&cfg.include).map_err(|e| GitHubError::Api {
        status: 400,
        body: format!("include glob: {e}"),
    })?;
    let exclude = cfg
        .exclude
        .as_deref()
        .map(|p| {
            compile_glob(p).map_err(|e| GitHubError::Api {
                status: 400,
                body: format!("exclude glob: {e}"),
            })
        })
        .transpose()?;
    let matched: Vec<BlobRef> = tree
        .into_iter()
        .filter(|b| {
            b.path.ends_with(".md")
                && include.is_match(&b.path)
                && exclude.as_ref().is_none_or(|m| !m.is_match(&b.path))
        })
        .collect();
    outcome.total_files = matched.len();

    // 增量下载（raw CDN，并发 6）
    let cache_root = state.cache_dir(&cfg.id);
    let mut new_shas: HashMap<String, String> = HashMap::new();
    let mut entries: Vec<ManifestEntry> = Vec::with_capacity(matched.len());
    let synced_at = chrono::Utc::now().to_rfc3339();

    let mut changed: Vec<&BlobRef> = Vec::new();
    for b in &matched {
        new_shas.insert(b.path.clone(), b.sha.clone());
        if current.file_shas.get(&b.path) != Some(&b.sha) {
            changed.push(b);
        }
    }
    let arc = Arc::clone(state);
    for chunk in changed.chunks(6) {
        let futs: Vec<_> = chunk
            .iter()
            .map(|b| {
                let arc = arc.clone();
                let (owner, repo, sha, path) = (
                    cfg.owner.clone(),
                    cfg.repo.clone(),
                    head_sha.clone(),
                    b.path.clone(),
                );
                async move {
                    let content = arc.gh.fetch_raw(&owner, &repo, &sha, &path).await;
                    (path, content)
                }
            })
            .collect();
        let results = futures_all(futs).await;
        for (path, res) in results {
            let content = res?;
            let file = cache_root.join(&path);
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).map_err(io_to_gh)?;
            }
            std::fs::write(&file, &content).map_err(io_to_gh)?;
            outcome.downloaded += 1;
            entries.push(build_entry(cfg, &resolved_ref, &path, &content, &synced_at));
        }
    }

    // 未变化的文件：从旧 entries 复制（metadata 不变）
    let changed_paths: HashSet<String> = entries.iter().map(|e| e.path.clone()).collect();
    for old in &current.entries {
        if new_shas.contains_key(&old.path) && !changed_paths.contains(&old.path) {
            entries.push(old.clone());
        }
    }

    // 删除已移除的缓存文件
    for old_path in current.file_shas.keys() {
        if !new_shas.contains_key(old_path) {
            let f = cache_root.join(old_path);
            if f.exists() {
                let _ = std::fs::remove_file(&f);
                outcome.removed += 1;
            }
        }
    }
    // 清空目录（尽力而为）
    prune_empty_dirs(&cache_root);

    // 写回 state
    {
        let mut runtime = state.runtime.lock().await;
        if let Some(rt) = runtime.get_mut(&cfg.id) {
            rt.state.last_sha = Some(head_sha);
            rt.state.resolved_ref = Some(resolved_ref);
            rt.state.file_shas = new_shas;
            rt.state.entries = entries;
        }
    }
    Ok(())
}

fn io_to_gh(e: std::io::Error) -> GitHubError {
    GitHubError::Api {
        status: 500,
        body: e.to_string(),
    }
}

/// 删除 cache_root 下空目录（自底向上，尽力而为）。
fn prune_empty_dirs(root: &Path) {
    fn rec(dir: &Path, root: &Path) -> bool {
        // 返回 dir 是否已空（可删）
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return false,
        };
        let mut empty = true;
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if !rec(&p, root) {
                    empty = false;
                }
            } else {
                empty = false;
            }
        }
        if empty && dir != root {
            let _ = std::fs::remove_dir(dir);
        }
        empty
    }
    if root.exists() {
        rec(root, root);
    }
}

/// 简易并发执行器（避免引 futures crate；JoinHandle 全收集）。
async fn futures_all<F>(futs: Vec<F>) -> Vec<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let mut handles = Vec::with_capacity(futs.len());
    for f in futs {
        handles.push(tokio::spawn(f));
    }
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        out.push(h.await.unwrap_or_else(|_| panic!("sync worker panicked")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_removes_empty_dirs() {
        let tmp = std::env::temp_dir().join(format!("gaubeeos-test-prune-{}", std::process::id()));
        let deep = tmp.join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("x.md"), "hi").unwrap();
        prune_empty_dirs(&tmp);
        assert!(deep.join("x.md").exists());
        std::fs::remove_file(deep.join("x.md")).unwrap();
        prune_empty_dirs(&tmp);
        assert!(!tmp.join("a").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
