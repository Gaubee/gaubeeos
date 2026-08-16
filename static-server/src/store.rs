//! managerStoreService：管理员站点级 KV 存储（namespace = appId，2026-08-17）。
//!
//! 双后端同接口（env 切换，MANAGER_STORE_MODE=local|git）：
//! - local（默认）：{DATA_DIR}/store/{ns}.json 原子写，权威数据。
//! - git：本地镜像 + GitHub 仓库文件夹（MANAGER_STORE_REPO / MANAGER_STORE_PATH）。
//!   内存先行（PUT 即生效）→ 本地镜像同步写 → per-ns 3s 防抖 commit
//!   （Contents API，sha 乐观锁，409 重拉重试一次）。
//!   写入凭据 = GITHUB_TOKEN env（机器笔）。崩溃恢复：本地镜像为缓冲真相，
//!   dirty 标记存在时镜像胜出并重排 flush。
//!
//! 配额 5MB/namespace（序列化字节数，PUT 时 413）；值限定 JSON 文档。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::github::GitHubClient;

pub const QUOTA_BYTES: usize = 5 * 1024 * 1024;
/// 防抖窗口：PUT 后静默 3s 才 commit（聚合高频写，如主题滑杆）。
pub const FLUSH_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(3);
/// 保留的 namespace（路由冲突 + 元信息）。
pub const RESERVED_NS: &[&str] = &["usage"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    Local,
    Git,
}

/// namespace 校验：^[a-z0-9-]{1,64}$ 且非保留字（防路径穿越）。
pub fn valid_ns(ns: &str) -> bool {
    !ns.is_empty()
        && ns.len() <= 64
        && !RESERVED_NS.contains(&ns)
        && ns
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub struct StoreManager {
    mode: StoreMode,
    /// local 权威目录 / git 本地镜像目录（同一布局：{dir}/{ns}.json）。
    dir: PathBuf,
    /// 内存态（两模式共用快照；读路径走这里）。
    mem: Mutex<HashMap<String, Value>>,
    /// git 模式：per-ns 写代数（防抖只 flush 最新一代；短临界区纯内存锁）。
    generations: std::sync::Mutex<HashMap<String, u32>>,
    /// git 配置。
    repo: Option<(String, String)>, // (owner, repo)
    folder: String,
    gh: GitHubClient,
}

pub struct StoreUsage {
    pub ns: String,
    pub bytes: usize,
}

impl StoreManager {
    pub fn from_env(data_dir: &std::path::Path, gh: GitHubClient) -> Arc<Self> {
        let mode = match std::env::var("MANAGER_STORE_MODE")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "git" => StoreMode::Git,
            _ => StoreMode::Local,
        };
        let dir = data_dir.join("store");
        let repo = std::env::var("MANAGER_STORE_REPO")
            .ok()
            .and_then(|s| {
                s.trim()
                    .split_once('/')
                    .map(|(o, r)| (o.trim().to_string(), r.trim().to_string()))
            })
            .filter(|(o, r)| !o.is_empty() && !r.is_empty());
        let folder = std::env::var("MANAGER_STORE_PATH")
            .unwrap_or_else(|_| "gaubeeos-store".into())
            .trim()
            .trim_matches('/')
            .to_string();
        if mode == StoreMode::Git && (repo.is_none() || gh_token_absent()) {
            tracing::warn!(
                "MANAGER_STORE_MODE=git 但 MANAGER_STORE_REPO/GITHUB_TOKEN 未配置，回落 local 模式"
            );
            return Arc::new(Self::new(StoreMode::Local, dir, None, folder, gh));
        }
        let _ = std::fs::create_dir_all(&dir);
        Arc::new(Self::new(mode, dir, repo, folder, gh))
    }

    fn new(
        mode: StoreMode,
        dir: PathBuf,
        repo: Option<(String, String)>,
        folder: String,
        gh: GitHubClient,
    ) -> Self {
        Self {
            mode,
            dir,
            mem: Mutex::new(HashMap::new()),
            generations: std::sync::Mutex::new(HashMap::new()),
            repo,
            folder,
            gh,
        }
    }

    pub fn mode(&self) -> StoreMode {
        self.mode
    }

    /// boot：加载 store（local 扫目录；git 拉远端 + 镜像恢复）。
    pub async fn boot(self: &Arc<Self>) {
        let mirror: HashMap<String, Value> = scan_dir_json(&self.dir);
        match self.mode {
            StoreMode::Local => {
                let n = mirror.len();
                *self.mem.lock().await = mirror;
                tracing::info!("store(local)：载入 {n} 个 namespace");
            }
            StoreMode::Git => {
                let Some((owner, repo)) = self.repo.clone() else {
                    return;
                };
                let mut remote: HashMap<String, Value> = HashMap::new();
                match self.gh.list_contents_dir(&owner, &repo, &self.folder).await {
                    Ok(files) => {
                        for (name, sha) in files {
                            let ns = name.strip_suffix(".json").unwrap_or(&name).to_string();
                            if !valid_ns(&ns) {
                                continue;
                            }
                            if let Ok(text) = self
                                .gh
                                .fetch_raw(
                                    &owner,
                                    &repo,
                                    &sha,
                                    &format!("{}/{}", self.folder, name),
                                )
                                .await
                            {
                                if let Ok(v) = serde_json::from_str(&text) {
                                    remote.insert(ns, v);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("store(git)：远端拉取失败（{}），使用本地镜像", e);
                    }
                }
                let mut mem = self.mem.lock().await;
                let mut pending_flush = Vec::new();
                for (ns, value) in mirror.iter() {
                    let dirty = self.dir.join(format!(".dirty-{ns}")).exists();
                    match remote.get(ns) {
                        // 镜像与远端一致：无事
                        Some(r) if *r == *value => {}
                        // 镜像有 dirty 标记（上次 flush 未完成）：镜像胜出，重排 flush
                        _ if dirty => {
                            mem.insert(ns.clone(), value.clone());
                            write_json_atomic(&self.dir, ns, value);
                            pending_flush.push(ns.clone());
                        }
                        // 远端较新（外部编辑）或本地无：远端胜出
                        _ => {}
                    }
                }
                for (ns, value) in remote {
                    write_json_atomic(&self.dir, &ns, &value);
                    mem.insert(ns, value);
                }
                let n = mem.len();
                drop(mem);
                for ns in pending_flush {
                    self.mark_dirty(&ns);
                    self.schedule_flush(&ns);
                }
                tracing::info!("store(git)：载入 {n} 个 namespace（{}/{}）", owner, repo);
            }
        }
    }

    /// 读（内存态）。
    pub async fn get(&self, ns: &str) -> Option<Value> {
        self.mem.lock().await.get(ns).cloned()
    }

    /// 写：配额校验 → 内存 → 本地（权威/镜像）→ git 模式防抖 flush。
    pub async fn put(self: &Arc<Self>, ns: &str, value: Value) -> Result<(), String> {
        if !valid_ns(ns) {
            return Err(format!(
                "非法 namespace：{ns}（^[a-z0-9-]{{1,64}}$，非保留字）"
            ));
        }
        let serialized = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        if serialized.len() > QUOTA_BYTES {
            return Err(format!(
                "超出配额：{} bytes > {} bytes（namespace {ns}）",
                serialized.len(),
                QUOTA_BYTES
            ));
        }
        {
            let mut mem = self.mem.lock().await;
            mem.insert(ns.to_string(), value.clone());
        }
        write_json_atomic(&self.dir, ns, &value);
        if self.mode == StoreMode::Git {
            self.mark_dirty(ns);
            self.schedule_flush(ns);
        }
        Ok(())
    }

    /// 删除 namespace（本地 + git 提交删除）。
    pub async fn remove(self: &Arc<Self>, ns: &str) -> Result<(), String> {
        if !valid_ns(ns) {
            return Err(format!("非法 namespace：{ns}"));
        }
        self.mem.lock().await.remove(ns);
        let _ = std::fs::remove_file(self.dir.join(format!("{ns}.json")));
        if self.mode == StoreMode::Git {
            if let Some((owner, repo)) = self.repo.clone() {
                let path = format!("{}/{}.json", self.folder, ns);
                if let Ok(Some(sha)) = self.gh.get_contents_meta(&owner, &repo, &path).await {
                    let _ = self
                        .gh
                        .delete_contents(&owner, &repo, &path, &format!("store: remove {ns}"), &sha)
                        .await;
                }
            }
        }
        Ok(())
    }

    /// 用量（序列化字节数）。
    pub async fn usage(&self) -> Vec<StoreUsage> {
        let mem = self.mem.lock().await;
        let mut out: Vec<StoreUsage> = mem
            .iter()
            .map(|(ns, v)| StoreUsage {
                ns: ns.clone(),
                bytes: serde_json::to_vec(v).map(|b| b.len()).unwrap_or(0),
            })
            .collect();
        out.sort_by(|a, b| a.ns.cmp(&b.ns));
        out
    }

    fn mark_dirty(&self, ns: &str) {
        let _ = std::fs::write(self.dir.join(format!(".dirty-{ns}")), b"1");
    }

    /// 防抖 flush：写代数 +1 并捕获；3s 后若仍是最新一代 → 提交（新写会取消旧任务）。
    fn schedule_flush(self: &Arc<Self>, ns: &str) {
        let my_gen = {
            let mut gens = self.generations.lock().unwrap();
            let g = gens.entry(ns.to_string()).or_insert(0);
            *g += 1;
            *g
        };
        let this = Arc::clone(self);
        let ns = ns.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(FLUSH_DEBOUNCE).await;
            let cur = this
                .generations
                .lock()
                .unwrap()
                .get(&ns)
                .copied()
                .unwrap_or(0);
            if cur == my_gen {
                this.flush(&ns).await;
            }
        });
    }

    async fn flush(&self, ns: &str) {
        let Some((owner, repo)) = self.repo.clone() else {
            return;
        };
        let Some(value) = self.mem.lock().await.get(ns).cloned() else {
            return;
        };
        let path = format!("{}/{}.json", self.folder, ns);
        let content = serde_json::to_string(&value).unwrap_or_default();
        let b64 = base64_encode(content.as_bytes());
        let message = format!("store: update {ns}");
        // sha 乐观锁：先查现 sha，PUT 409 时重查重试一次
        for attempt in 0..2 {
            let sha = self
                .gh
                .get_contents_meta(&owner, &repo, &path)
                .await
                .ok()
                .flatten();
            match self
                .gh
                .put_contents(&owner, &repo, &path, &message, &b64, sha.as_deref())
                .await
            {
                Ok(()) => {
                    let _ = std::fs::remove_file(self.dir.join(format!(".dirty-{ns}")));
                    tracing::info!("store(git)：已提交 {ns}");
                    return;
                }
                Err(e) => {
                    tracing::warn!(
                        "store(git)：{ns} flush 失败（attempt {}）：{e}",
                        attempt + 1
                    );
                }
            }
        }
    }
}

fn gh_token_absent() -> bool {
    std::env::var("GITHUB_TOKEN")
        .map(|t| t.trim().is_empty())
        .unwrap_or(true)
}

/// 扫描目录下全部 {ns}.json（.dirty-* 与非法名跳过）。
pub fn scan_dir_json(dir: &std::path::Path) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(ns) = name.strip_suffix(".json") {
                if valid_ns(ns) {
                    if let Ok(text) = std::fs::read_to_string(e.path()) {
                        if let Ok(v) = serde_json::from_str(&text) {
                            out.insert(ns.to_string(), v);
                        }
                    }
                }
            }
        }
    }
    out
}

/// 原子写 JSON 文件（tmp + rename）。
pub fn write_json_atomic(dir: &std::path::Path, ns: &str, value: &Value) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let tmp = dir.join(format!("{ns}.json.tmp"));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(format!("{ns}.json")));
        }
    }
}

/// 标准 base64 编码（Contents API 需要）。
pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ns_validation() {
        assert!(valid_ns("theme"));
        assert!(valid_ns("desktop-2"));
        assert!(!valid_ns(""));
        assert!(!valid_ns("Theme"));
        assert!(!valid_ns("a/b"));
        assert!(!valid_ns(".."));
        assert!(!valid_ns("usage")); // 保留字
        assert!(!valid_ns(&"x".repeat(65)));
    }

    #[test]
    fn base64_roundtrip_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[tokio::test]
    async fn local_put_get_quota_and_usage() {
        let dir = std::env::temp_dir().join(format!("gaubeeos-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gh = GitHubClient::new(None);
        let store = Arc::new(StoreManager::new(
            StoreMode::Local,
            dir.clone(),
            None,
            "s".into(),
            gh,
        ));
        store.boot().await;

        store
            .put("theme", serde_json::json!({"hue": 200}))
            .await
            .unwrap();
        assert_eq!(
            store.get("theme").await,
            Some(serde_json::json!({"hue": 200}))
        );
        // 权威文件落盘
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("theme.json")).unwrap())
                .unwrap();
        assert_eq!(on_disk, serde_json::json!({"hue": 200}));

        // 非法 ns / 保留字
        assert!(store.put("../evil", serde_json::json!({})).await.is_err());
        assert!(store.put("usage", serde_json::json!({})).await.is_err());

        // 配额：>5MB 拒绝
        let big = "x".repeat(QUOTA_BYTES + 10);
        assert!(store.put("big", serde_json::json!(big)).await.is_err());

        // usage
        let usage = store.usage().await;
        assert!(usage.iter().any(|u| u.ns == "theme" && u.bytes > 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn boot_reload_from_disk() {
        let dir =
            std::env::temp_dir().join(format!("gaubeeos-store-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_json_atomic(&dir, "a", &serde_json::json!([1, 2]));
        let store = Arc::new(StoreManager::new(
            StoreMode::Local,
            dir.clone(),
            None,
            "s".into(),
            GitHubClient::new(None),
        ));
        store.boot().await;
        assert_eq!(store.get("a").await, Some(serde_json::json!([1, 2])));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
