//! 后端会话与 Manager 权限（2026-08-17）。
//!
//! 模型：GitHub OAuth 是唯一可信源。前端拿 gh token 调 POST /api/session 交换，
//! 后端经 GET /user 验证后签发服务端会话（HttpOnly cookie），gh token 不再出现。
//! role 每请求按 env 重算：login ∈ MANAGER_GITHUB_ACCOUNTS（逗号分隔、大小写不敏感）。
//!
//! env：
//! - MANAGER_GITHUB_ACCOUNTS：管理员 GitHub login 列表（逗号分隔）
//! - DEV_SESSION_TOKEN：本地开发/演示用——boot 时直接注册为 manager 会话
//!   （浏览器手动 document.cookie 即可测管理流，生产勿配）
//! - COOKIE_SECURE：非空则 cookie 加 Secure（https 部署；本地 http 不加）

use std::collections::HashMap;

use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::sync::AppState;

pub const SESSION_COOKIE: &str = "gaubeeos_session";
/// 会话有效期（7 天，滑动续期由再次访问实现：map 常驻，cookie Max-Age 刷新）。
pub const SESSION_MAX_AGE: i64 = 7 * 24 * 3600;

/// 一条服务端会话。
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub login: String,
    /// 审计信息（保留；当前无消费方）。
    #[allow(dead_code)]
    pub created_at: String,
}

/// 请求身份（中间件注入 request extensions；login 为 None = 匿名）。
#[derive(Debug, Clone, Default)]
pub struct RequestIdentity {
    pub login: Option<String>,
}

impl RequestIdentity {
    pub fn role(&self) -> &'static str {
        role_of(self.login.as_deref())
    }
}

/// 管理员 login 列表（小写）。
pub fn manager_logins() -> Vec<String> {
    std::env::var("MANAGER_GITHUB_ACCOUNTS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 是否管理员（大小写不敏感）。
pub fn is_manager(login: &str) -> bool {
    let l = login.trim().to_ascii_lowercase();
    manager_logins().contains(&l)
}

/// 角色字符串（"manager" | "user" | "anonymous"）。
pub fn role_of(login: Option<&str>) -> &'static str {
    match login {
        Some(l) if is_manager(l) => "manager",
        Some(_) => "user",
        None => "anonymous",
    }
}

/// 解析 Cookie 头里的会话 token。
pub fn session_token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    for value in headers.get_all(axum::http::header::COOKIE) {
        let s = value.to_str().ok()?;
        for pair in s.split(';') {
            let pair = pair.trim();
            if let Some(rest) = pair.strip_prefix(SESSION_COOKIE) {
                if let Some(tok) = rest.strip_prefix('=') {
                    let tok = tok.trim_matches('"');
                    if !tok.is_empty() {
                        return Some(tok.to_string());
                    }
                }
            }
        }
    }
    None
}

/// 会话中间件：cookie → 查会话表 → 身份注入 request extensions（不拦截，判定交给 handler）。
pub async fn session_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let identity = match session_token_from_headers(req.headers()) {
        Some(tok) => {
            let sessions = state.sessions.lock().await;
            RequestIdentity {
                login: sessions.get(&tok).map(|e| e.login.clone()),
            }
        }
        None => RequestIdentity::default(),
    };
    req.extensions_mut().insert(identity);
    next.run(req).await
}

use std::sync::Arc;

/// 生成 32 字节随机会话 token（hex）。
pub fn generate_session_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).expect("getrandom");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// 构造 Set-Cookie 值。
pub fn build_session_cookie(token: &str) -> String {
    let secure = if std::env::var("COOKIE_SECURE").is_ok() {
        "; Secure"
    } else {
        ""
    };
    format!("{SESSION_COOKIE}={token}; Path=/api; HttpOnly; SameSite=Lax{secure}; Max-Age={SESSION_MAX_AGE}")
}

/// 过期 cookie（登出用）。
pub fn expire_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/api; HttpOnly; SameSite=Lax; Max-Age=0")
}

/// boot 时注册 dev 会话（DEV_SESSION_TOKEN，仅显式配置时）。
pub fn seed_dev_session(sessions: &mut HashMap<String, SessionEntry>) {
    if let Ok(tok) = std::env::var("DEV_SESSION_TOKEN") {
        let tok = tok.trim().to_string();
        if tok.is_empty() {
            return;
        }
        let managers = manager_logins();
        let login = managers
            .first()
            .cloned()
            .unwrap_or_else(|| "dev-manager".into());
        tracing::warn!(
            "DEV_SESSION_TOKEN 已配置：已注册开发用 manager 会话（login={login}）。生产环境请勿配置。"
        );
        sessions.insert(
            tok,
            SessionEntry {
                login,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_match_case_insensitive() {
        std::env::set_var("MANAGER_GITHUB_ACCOUNTS", "Gaubee, someone-else");
        assert!(is_manager("gaubee"));
        assert!(is_manager("GAUBEE"));
        assert!(is_manager(" Someone-Else "));
        assert!(!is_manager("other"));
        std::env::remove_var("MANAGER_GITHUB_ACCOUNTS");
    }

    #[test]
    fn role_of_variants() {
        std::env::set_var("MANAGER_GITHUB_ACCOUNTS", "gaubee");
        assert_eq!(role_of(Some("gaubee")), "manager");
        assert_eq!(role_of(Some("visitor")), "user");
        assert_eq!(role_of(None), "anonymous");
        std::env::remove_var("MANAGER_GITHUB_ACCOUNTS");
    }

    #[test]
    fn cookie_parsing() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "other=1; gaubeeos_session=abc123; x=y".parse().unwrap(),
        );
        assert_eq!(
            session_token_from_headers(&headers).as_deref(),
            Some("abc123")
        );
        let empty = axum::http::HeaderMap::new();
        assert_eq!(session_token_from_headers(&empty), None);
    }

    #[test]
    fn cookie_attributes() {
        let c = build_session_cookie("tok");
        assert!(c.contains("Path=/api"));
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("SameSite=Lax"));
        assert!(c.contains("Max-Age=604800"));
    }
}
