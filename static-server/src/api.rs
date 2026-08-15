//! REST API（/api/*，与静态服务同进程同端口）。
//!
//! - GET    /api/sources            订阅列表（配置 + 运行态）
//! - POST   /api/sources            新增订阅（校验 + 落盘 + 调度 + 立即同步）
//! - PUT    /api/sources/:id        全量更新（重新调度）
//! - DELETE /api/sources/:id        删除（取消调度 + 清缓存 + 重建清单）
//! - PATCH  /api/sources/:id/enabled 启停（便捷开关）
//! - POST   /api/sources/:id/sync   手动同步（等待完成返回结果）
//! - POST   /api/sources/test       测试连接（head + trees + glob 命中预览，不下载）
//! - GET    /api/content/manifest   聚合清单（内存态，缺失时读盘）
//! - GET    /api/content/file?uid=  单篇 markdown 正文

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{compile_glob, save_config, Collection, SourceConfig, ValidationError};
use crate::manifest::{Manifest, ManifestEntry};
use crate::sync::{
    schedule_source, stop_source, sync_once, unschedule_source, AppState, SourceState,
};

pub fn api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sources", get(list_sources).post(create_source))
        .route(
            "/sources/{id}",
            axum::routing::put(update_source).delete(delete_source),
        )
        .route("/sources/{id}/sync", post(sync_source))
        .route("/sources/{id}/enabled", post(set_enabled))
        .route("/sources/test", post(test_source))
        .route("/content/manifest", get(get_manifest))
        .route("/content/file", get(get_file))
}

// ---- DTO ----

#[derive(Serialize)]
struct SourceDto {
    #[serde(flatten)]
    config: SourceConfig,
    /// 便捷展示名（name 缺省用 owner/repo）。
    display_name: String,
    state: SourceState,
}

async fn list_sources(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let cfg = state.config.read().await.clone();
    let runtime = state.runtime.lock().await;
    let sources: Vec<SourceDto> = cfg
        .sources
        .iter()
        .map(|c| SourceDto {
            display_name: c
                .name
                .clone()
                .unwrap_or_else(|| format!("{}/{}", c.owner, c.repo)),
            state: runtime
                .get(&c.id)
                .map(|rt| rt.state.clone())
                .unwrap_or_default(),
            config: c.clone(),
        })
        .collect();
    Json(json!({ "sources": sources }))
}

/// 新增/更新共用的请求体（id 服务端派生，更新时以路径为准）。
#[derive(Deserialize)]
struct SourceInput {
    name: Option<String>,
    owner: String,
    repo: String,
    #[serde(rename = "ref", default)]
    git_ref: String,
    collection: Collection,
    include: String,
    exclude: Option<String>,
    slug_prefix: Option<String>,
    #[serde(default = "default_interval")]
    interval: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_interval() -> String {
    "1h".into()
}
fn default_true() -> bool {
    true
}

impl SourceInput {
    fn into_config(self, id: String) -> SourceConfig {
        SourceConfig {
            id,
            name: self.name,
            owner: self.owner,
            repo: self.repo,
            git_ref: self.git_ref,
            collection: self.collection,
            include: self.include,
            exclude: self.exclude,
            slug_prefix: self.slug_prefix,
            interval: self.interval,
            enabled: self.enabled,
        }
    }
}

fn bad_request(msg: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": msg.to_string() })),
    )
        .into_response()
}

async fn create_source(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SourceInput>,
) -> Response {
    let id = SourceConfig::derive_id(&input.owner, &input.repo, &input.git_ref, &input.include);
    let cfg = input.into_config(id);
    if let Err(e) = cfg.validate() {
        return bad_request(e);
    }
    {
        let mut config = state.config.write().await;
        if config.sources.iter().any(|s| s.id == cfg.id) {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": "同仓库同 include 的订阅已存在", "id": cfg.id })),
            )
                .into_response();
        }
        config.sources.push(cfg.clone());
        if let Err(e) = save_config(&state.data_dir.join("config.toml"), &config) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("配置落盘失败: {e}") })),
            )
                .into_response();
        }
    }
    if cfg.enabled {
        schedule_source(&state, cfg.clone()).await;
    } else {
        // 未启用的源也要占位（state 空壳），列表页可见
        state
            .runtime
            .lock()
            .await
            .entry(cfg.id.clone())
            .or_insert_with(|| crate::sync::SourceRuntime {
                task: tokio::spawn(std::future::pending()),
                lock: Arc::new(tokio::sync::Mutex::new(())),
                state: SourceState::default(),
            });
    }
    // 立即同步一次并返回结果（前台展示首轮同步状态）
    let outcome = if cfg.enabled {
        Some(sync_once(&state, &cfg).await)
    } else {
        None
    };
    (
        StatusCode::CREATED,
        Json(json!({ "id": cfg.id, "outcome": outcome })),
    )
        .into_response()
}

async fn update_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<SourceInput>,
) -> Response {
    let cfg = input.into_config(id.clone());
    if let Err(e) = cfg.validate() {
        return bad_request(e);
    }
    {
        let mut config = state.config.write().await;
        let Some(slot) = config.sources.iter_mut().find(|s| s.id == id) else {
            return not_found(id);
        };
        *slot = cfg.clone();
        if let Err(e) = save_config(&state.data_dir.join("config.toml"), &config) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("配置落盘失败: {e}") })),
            )
                .into_response();
        }
    }
    // 配置变了（include/ref 等）：作废旧 state 强制全量重同步
    unschedule_source(&state, &id).await;
    let _ = std::fs::remove_file(state.data_dir.join("state").join(format!("{id}.json")));
    if cfg.enabled {
        schedule_source(&state, cfg.clone()).await;
    }
    Json(json!({ "id": id, "resynced": true })).into_response()
}

async fn delete_source(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    {
        let mut config = state.config.write().await;
        let before = config.sources.len();
        config.sources.retain(|s| s.id != id);
        if config.sources.len() == before {
            return not_found(id);
        }
        if let Err(e) = save_config(&state.data_dir.join("config.toml"), &config) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("配置落盘失败: {e}") })),
            )
                .into_response();
        }
    }
    unschedule_source(&state, &id).await;
    let _ = std::fs::remove_file(state.data_dir.join("state").join(format!("{id}.json")));
    let _ = std::fs::remove_dir_all(state.data_dir.join("cache").join("sources").join(&id));
    let _ = state.rebuild_manifest().await;
    StatusCode::NO_CONTENT.into_response()
}

async fn set_enabled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(enabled) = body.get("enabled").and_then(|v| v.as_bool()) else {
        return bad_request("body 需要 { enabled: boolean }");
    };
    let cfg = {
        let mut config = state.config.write().await;
        let Some(slot) = config.sources.iter_mut().find(|s| s.id == id) else {
            return not_found(id);
        };
        slot.enabled = enabled;
        let cfg = slot.clone();
        if let Err(e) = save_config(&state.data_dir.join("config.toml"), &config) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("配置落盘失败: {e}") })),
            )
                .into_response();
        }
        cfg
    };
    if enabled {
        schedule_source(&state, cfg).await;
    } else {
        // 停用 = 停止同步调度，内容保留（清单仍聚合该源 entries）
        stop_source(&state, &id).await;
    }
    Json(json!({ "id": id, "enabled": enabled })).into_response()
}

async fn sync_source(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let cfg = {
        let config = state.config.read().await;
        match config.sources.iter().find(|s| s.id == id) {
            Some(c) => c.clone(),
            None => return not_found(id),
        }
    };
    let outcome = sync_once(&state, &cfg).await;
    Json(outcome).into_response()
}

/// 测试连接：跑 head + trees + glob 过滤，返回命中预览（不下载）。
#[derive(Deserialize)]
struct TestInput {
    owner: String,
    repo: String,
    #[serde(rename = "ref", default)]
    git_ref: String,
    include: String,
    exclude: Option<String>,
}

#[derive(Serialize)]
struct TestResult {
    head_sha: String,
    resolved_ref: String,
    matched: usize,
    sample: Vec<String>,
}

async fn test_source(State(state): State<Arc<AppState>>, Json(input): Json<TestInput>) -> Response {
    if input.owner.trim().is_empty() || input.repo.trim().is_empty() {
        return bad_request(ValidationError::MissingRepo);
    }
    let Ok(include) = compile_glob(&input.include) else {
        return bad_request(ValidationError::BadInclude(input.include));
    };
    let exclude = match input.exclude.as_deref().map(compile_glob) {
        None => None,
        Some(Ok(m)) => Some(m),
        Some(Err(_)) => {
            return bad_request(ValidationError::BadExclude(
                input.exclude.unwrap_or_default(),
            ))
        }
    };
    let (head_sha, resolved_ref) = match state
        .gh
        .resolve_head(&input.owner, &input.repo, &input.git_ref)
        .await
    {
        Ok(v) => v,
        Err(e) => return bad_request(e),
    };
    let tree = match state
        .gh
        .fetch_tree(&input.owner, &input.repo, &head_sha)
        .await
    {
        Ok(t) => t,
        Err(e) => return bad_request(e),
    };
    let mut matched: Vec<String> = tree
        .into_iter()
        .map(|b| b.path)
        .filter(|p| {
            p.ends_with(".md")
                && include.is_match(p)
                && exclude.as_ref().is_none_or(|m| !m.is_match(p))
        })
        .collect();
    let total = matched.len();
    matched.truncate(10);
    Json(TestResult {
        head_sha,
        resolved_ref,
        matched: total,
        sample: matched,
    })
    .into_response()
}

async fn get_manifest(State(state): State<Arc<AppState>>) -> Response {
    let m: Manifest = {
        let runtime = state.runtime.lock().await;
        if runtime.values().any(|rt| !rt.state.entries.is_empty()) {
            let all: Vec<ManifestEntry> = runtime
                .values()
                .flat_map(|rt| rt.state.entries.clone())
                .collect();
            drop(runtime);
            crate::manifest::aggregate(all)
        } else {
            drop(runtime);
            match std::fs::read(state.data_dir.join("cache").join("manifest.json")) {
                Ok(bytes) => match serde_json::from_slice(&bytes) {
                    Ok(m) => m,
                    Err(_) => crate::manifest::aggregate(vec![]),
                },
                Err(_) => crate::manifest::aggregate(vec![]),
            }
        }
    };
    Json(m).into_response()
}

#[derive(Deserialize)]
struct FileQuery {
    uid: String,
}

async fn get_file(State(state): State<Arc<AppState>>, Query(q): Query<FileQuery>) -> Response {
    // uid = {sourceId}:{path}
    let Some((source_id, path)) = q.uid.split_once(':') else {
        return bad_request("uid 格式应为 {sourceId}:{path}");
    };
    // 路径安全：拒绝 `..` 段与绝对路径（防穿越）
    if path.starts_with('/') || path.split('/').any(|seg| seg == "..") {
        return bad_request("非法路径");
    }
    let file = state
        .data_dir
        .join("cache")
        .join("sources")
        .join(source_id)
        .join(path);
    match tokio::fs::read(&file).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
            bytes,
        )
            .into_response(),
        Err(_) => not_found(q.uid),
    }
}

fn not_found(what: impl std::fmt::Display) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": format!("不存在: {what}") })),
    )
        .into_response()
}
