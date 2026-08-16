//! GaubeeOS 服务端（axum + tokio，2026-08-16）。
//!
//! 正交意图：
//! 1. 静态托管（继承自 gaubee-static-server）：四级查找语义与退役的 nginx
//!    try_files 对齐（`$uri`/`{path}/index.html`/`{path}.html`/SPA fallback），
//!    缓存矩阵（no-cache 协商 / immutable 一年 / .md MIME）+ gzip。
//!    历史决策见 git 历史（Pingora 是代理框架无静态模块，故选 axum + tower-http）。
//! 2. 内容订阅引擎（本版核心）：GitHub 源 → 本地缓存 → /api/*。
//!    有状态：配置与缓存落 `{DATA_DIR}`（容器挂卷持久化）。
//! 3. 探活：/healthz（scratch 镜像无 shell，exec 型 healthcheck 不可用）。
//!
//! 环境变量：
//! - SERVER_ROOT（默认 /srv）静态产物根
//! - PORT（默认 8080）
//! - DATA_DIR（默认 /data；本地开发设 ./.data）
//! - GITHUB_TOKEN（可选，优先于 config.toml 的 github_token）

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tower::ServiceExt;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};

mod api;
mod config;
mod github;
mod manifest;
mod seo;
mod session;
mod store;
mod sync;

use sync::AppState;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    let root = PathBuf::from(std::env::var("SERVER_ROOT").unwrap_or_else(|_| "/srv".into()));
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let data_dir = PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".into()));
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    // ---- 订阅引擎 bootstrap ----
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        tracing::warn!("DATA_DIR 创建失败（可能只读或权限问题）: {e}");
    }
    let disk_config = config::load_config(&data_dir.join("config.toml")).unwrap_or_else(|e| {
        tracing::error!("config.toml 解析失败，按空配置启动: {e}");
        config::AppConfig::default()
    });
    let token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| disk_config.github_token.clone());

    let state = Arc::new(AppState::new(data_dir.clone(), token));
    *state.config.write().await = disk_config.clone();

    state.store.boot().await;
    tracing::info!(
        "managerStore：模式 = {:?}（namespace=appId，配额 5MB；env MANAGER_STORE_MODE 可切 git）",
        state.store.mode()
    );
    let ids: Vec<String> = disk_config.sources.iter().map(|s| s.id.clone()).collect();
    state.restore_states(&ids).await;
    let enabled: Vec<config::SourceConfig> = disk_config
        .sources
        .iter()
        .filter(|s| s.enabled)
        .cloned()
        .collect();
    for cfg in enabled {
        tracing::info!("调度订阅源 {} ({}/{})", cfg.id, cfg.owner, cfg.repo);
        sync::schedule_source(&state, cfg).await;
    }
    tracing::info!(
        "gaubeeos-server: 静态根={}? 订阅源 {} 个（启用 {}），DATA_DIR={}",
        root.display(),
        disk_config.sources.len(),
        disk_config.sources.iter().filter(|s| s.enabled).count(),
        data_dir.display()
    );

    // ---- HTTP 装配 ----
    // 静态主体：append_index=false（关闭目录 307 与自动 index），
    // SSG 双格式与 SPA 兜底收口到 fallback 三级查找。
    // 注意用 .fallback() 而非 .not_found_service()：后者把响应状态码强制改 404。
    let root_for_log = root.display().to_string();
    let state_for_fallback = Arc::clone(&state);
    let serve_dir = ServeDir::new(&root)
        .append_index_html_on_directories(false)
        .fallback(tower::service_fn(move |req: Request<axum::body::Body>| {
            fallback(req, root.clone(), Arc::clone(&state_for_fallback))
        }));

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/robots.txt",
            get(|State(app): State<Arc<AppState>>| async move {
                let site = app.config.read().await.site.clone();
                (
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    seo::render_robots(&site),
                )
                    .into_response()
            }),
        )
        .route(
            "/sitemap.xml",
            get(|State(app): State<Arc<AppState>>| async move {
                let site = app.config.read().await.site.clone();
                let entries = entries_snapshot(&app).await;
                match seo::render_sitemap(&site, &entries) {
                    Some(xml) => (
                        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
                        xml,
                    )
                        .into_response(),
                    None => (
                        StatusCode::NOT_FOUND,
                        "sitemap 未启用：请在 设置 → 站点 配置 base_url",
                    )
                        .into_response(),
                }
            }),
        )
        .nest("/api", api::api_router())
        .fallback_service(serve_dir)
        .layer(middleware::from_fn(cache_and_mime))
        .layer(CompressionLayer::new())
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            session::session_middleware,
        ))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr} 失败：{e}"));
    tracing::info!("gaubeeos-server listening on {addr}, root={root_for_log}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("收到退出信号");
        })
        .await
        .expect("server error");
}

/// 三级 fallback（`$uri` 精确命中由 ServeDir 完成）：
/// 0. `/article/{collection}/{slug}`（HTML 请求）→ SEO meta shell（SPA 壳变换）
/// 1. `{path}.html` —— 扁平 SSG 格式
/// 2. `{path}/index.html` —— 目录式（含 `/` 根）
/// 3. `{root}/index.html` —— SPA fallback
async fn fallback(
    req: Request<axum::body::Body>,
    root: PathBuf,
    app: Arc<AppState>,
) -> Result<Response, std::convert::Infallible> {
    let raw = req.uri().path().to_owned();
    let path = raw.trim_matches('/').to_owned();

    // SEO meta shell：文章详情路由（零 UA 嗅探——人类与爬虫同 HTML）
    if req.method() == axum::http::Method::GET
        && (req
            .headers()
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("text/html"))
    {
        if let Some(resp) = try_article_shell(&raw, &root, &app).await {
            return Ok(resp);
        }
    }

    if !path.is_empty() {
        let flat = format!("{path}.html");
        if let Some(body) = read_file(&root, &flat).await {
            return Ok(file_response(&flat, body));
        }
    }

    let dir_index = if path.is_empty() {
        "index.html".to_owned()
    } else {
        format!("{path}/index.html")
    };
    if let Some(body) = read_file(&root, &dir_index).await {
        return Ok(file_response(&dir_index, body));
    }

    match ServeFile::new(root.join("index.html")).oneshot(req).await {
        Ok(resp) => Ok(resp.into_response()),
        Err(_) => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

/// 清单条目快照（runtime 聚合）。
async fn entries_snapshot(app: &Arc<AppState>) -> Vec<manifest::ManifestEntry> {
    let runtime = app.runtime.lock().await;
    runtime
        .values()
        .flat_map(|rt| rt.state.entries.clone())
        .collect()
}

/// `/article/{collection}/{slug}` → SEO meta shell。
/// 命中（slug = `{slug_prefix}{slug}`）→ index.html 变换返回；未命中 → None（走 SPA 兜底）。
async fn try_article_shell(
    raw_path: &str,
    root: &std::path::Path,
    app: &Arc<AppState>,
) -> Option<Response> {
    // 解析 /article/{collection}/{slug}
    let rest = raw_path.strip_prefix("/article/")?;
    let (collection, url_slug) = rest.split_once('/')?;
    if collection.is_empty() || url_slug.is_empty() || url_slug.contains('/') {
        return None;
    }
    // 查清单条目
    let entries = entries_snapshot(app).await;
    let entry = entries
        .iter()
        .find(|e| e.collection == collection && format!("{}{}", e.slug_prefix, e.slug) == url_slug)?
        .clone();
    // 读正文缓存
    let body_path = app
        .data_dir
        .join("cache")
        .join("sources")
        .join(&entry.source.id)
        .join(&entry.path);
    let body_md = tokio::fs::read_to_string(&body_path)
        .await
        .unwrap_or_default();
    // SPA 壳
    let index_html = String::from_utf8(read_file(root, "index.html").await?).ok()?;
    let site = app.config.read().await.site.clone();
    let html = seo::render_article_shell(&index_html, url_slug, &entry, &body_md, &site);
    Some(([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response())
}

/// 读取 root 下相对路径文件；ParentDir / 绝对路径段直接拒绝（防穿越）。
async fn read_file(root: &std::path::Path, rel: &str) -> Option<Vec<u8>> {
    use std::path::Component;
    let rel_path = PathBuf::from(rel);
    if rel_path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
    {
        return None;
    }
    tokio::fs::read(root.join(rel_path)).await.ok()
}

fn file_response(rel: &str, body: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(rel).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref().to_owned())], body).into_response()
}

/// 缓存矩阵 + MIME 修正（覆盖全部路由含 fallback 与 /api）：
/// - /api/* 不设强缓存（清单/正文总是协商最新）
/// - `/_app/immutable/*` → 一年 immutable（vite 内容哈希资产）
/// - 其余 → no-cache（协商缓存）
/// - `.md` → text/markdown
async fn cache_and_mime(req: Request<axum::body::Body>, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();

    if path.starts_with("/api/") {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    } else if path.starts_with("/_app/immutable/") {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    } else {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    if path.ends_with(".md") {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/markdown; charset=utf-8"),
        );
    }
    resp
}
