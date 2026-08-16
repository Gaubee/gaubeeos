//! 订阅配置（事实源：`{data_dir}/config.toml`）。
//!
//! 设计约定：
//! - 配置文件是唯一事实源：REST API 与手工编辑皆可，API 写入后原子落盘（tmp+rename）。
//! - 运行时状态（last sha / 已同步文件 / 错误）不属于配置，存 `{data_dir}/state/{id}.json`。
//! - `ref` 留空 = 跟随仓库默认分支（启动时解析并缓存）。
//! - `include/exclude` 为 glob（globset 语法：`**`、`*`、`?`、`{a,b}`）。

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 内容集合：与前端 articles/shout 两应用的 collection 对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Collection {
    Articles,
    Events,
}

impl Collection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Collection::Articles => "articles",
            Collection::Events => "events",
        }
    }
}

impl fmt::Display for Collection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 单个内容源订阅。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceConfig {
    pub id: String,
    /// 展示名（缺省用 `owner/repo`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub owner: String,
    pub repo: String,
    /// Git ref（分支/tag/sha）。空 = 默认分支。
    #[serde(rename = "ref", default)]
    pub git_ref: String,
    pub collection: Collection,
    /// 文件匹配 glob（仓库相对路径），如 `src/content/articles/**/*.md`。
    pub include: String,
    /// 排除 glob（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
    /// URL slug 前缀（多源同名文件防冲突用），默认空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug_prefix: Option<String>,
    /// 同步频率：humantime 语法（15m / 1h / 6h / 24h …）。
    #[serde(default = "default_interval")]
    pub interval: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_interval() -> String {
    "1h".to_string()
}

fn default_true() -> bool {
    true
}

impl SourceConfig {
    /// 解析 interval 为 Duration（非法值回退 1h，由 validate 提前拦截）。
    pub fn interval_duration(&self) -> Duration {
        humantime::parse_duration(&self.interval).unwrap_or(Duration::from_secs(3600))
    }
}

/// 底部状态栏外链（如 GitHub 源码入口、ICP 备案号）。
/// 备案合规场景：label=备案号，url=https://beian.miit.gov.cn/。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FooterLink {
    pub id: String,
    pub label: String,
    pub url: String,
}

/// 站点展示配置（部署者身份 + SEO，全站生效）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    /// 底部状态栏外链列表（空 = 不渲染任何链接）。
    #[serde(default)]
    pub footer_links: Vec<FooterLink>,
    /// 站点名（SEO title 模板后缀；空 = "GaubeeOS"）。
    #[serde(default)]
    pub site_name: String,
    /// 站点默认描述（meta description / og:description 兜底）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 站点绝对地址（https://example.com，无尾斜杠）。配置后启用
    /// canonical / og:url / robots.txt Sitemap 行 / sitemap.xml 生成。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Open Graph 分享图（绝对 http(s) URL，可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub og_image: Option<String>,
    /// 是否允许搜索引擎索引（false → robots.txt 全站 Disallow + noindex meta）。
    #[serde(default = "default_true")]
    pub allow_indexing: bool,
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            footer_links: Vec::new(),
            site_name: "GaubeeOS".to_string(),
            description: None,
            base_url: None,
            og_image: None,
            allow_indexing: true,
        }
    }
}

impl SiteConfig {
    /// 生效站点名（空配置回退默认）。
    pub fn name(&self) -> &str {
        if self.site_name.trim().is_empty() {
            "GaubeeOS"
        } else {
            &self.site_name
        }
    }

    /// 校验 SEO 相关字段（base_url/og_image 须为 http(s) 且无尾斜杠）。
    pub fn validate(&self) -> Result<(), ValidationError> {
        for (field, url) in [("base_url", &self.base_url), ("og_image", &self.og_image)] {
            if let Some(u) = url {
                let u = u.trim();
                if !(u.starts_with("http://") || u.starts_with("https://")) {
                    return Err(ValidationError::BadSiteField(format!(
                        "{field} 必须以 http(s):// 开头"
                    )));
                }
                if u.ends_with('/') && field == "base_url" {
                    return Err(ValidationError::BadSiteField(
                        "base_url 不应以 / 结尾（canonical 拼接约定）".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// 全局配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// GitHub API token（可选；环境变量 GITHUB_TOKEN 优先于配置文件）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_token: Option<String>,
    /// 站点展示配置（状态栏外链等）。
    #[serde(default)]
    pub site: SiteConfig,
    /// 订阅源列表。
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML 解析: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML 序列化: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

/// 配置校验错误（API 400 响应体）。
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("owner/repo 不能为空")]
    MissingRepo,
    #[error("include 不能为空")]
    MissingInclude,
    #[error("include 不是合法 glob: {0}")]
    BadInclude(String),
    #[error("exclude 不是合法 glob: {0}")]
    BadExclude(String),
    #[error("interval 无法解析（支持 15m/1h/6h/24h 等）: {0}")]
    BadInterval(String),
    #[error("站点配置字段不合法: {0}")]
    BadSiteField(String),
}

impl SourceConfig {
    /// 字段校验（glob 可编译、interval 可解析、repo 非空）。
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.owner.trim().is_empty() || self.repo.trim().is_empty() {
            return Err(ValidationError::MissingRepo);
        }
        if self.include.trim().is_empty() {
            return Err(ValidationError::MissingInclude);
        }
        compile_glob(&self.include).map_err(|e| ValidationError::BadInclude(e.to_string()))?;
        if let Some(ex) = &self.exclude {
            compile_glob(ex).map_err(|e| ValidationError::BadExclude(e.to_string()))?;
        }
        humantime::parse_duration(&self.interval)
            .map_err(|e| ValidationError::BadInterval(e.to_string()))?;
        Ok(())
    }

    /// 由仓库坐标 + include 派生稳定 id（同配置重添加得到同 id，幂等友好）。
    pub fn derive_id(owner: &str, repo: &str, git_ref: &str, include: &str) -> String {
        let mut hasher = DefaultHasher::new();
        (owner, repo, git_ref, include).hash(&mut hasher);
        format!("{owner}-{repo}-{:016x}", hasher.finish())
    }
}

/// 编译 glob（globset，路径匹配模式：`*` 不跨 `/`，`**` 跨）。
pub fn compile_glob(pattern: &str) -> Result<globset::GlobMatcher, globset::Error> {
    let gb = globset::GlobBuilder::new(pattern)
        .literal_separator(true) // `*` 不跨目录，与常见内容 glob 语义一致
        .build()?;
    Ok(gb.compile_matcher())
}

/// 从磁盘加载配置；文件不存在时返回空配置（首次启动）。
pub fn load_config(path: &Path) -> Result<AppConfig, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(e) => Err(e.into()),
    }
}

/// 原子写入配置（同目录 tmp + rename，避免半写状态被读到）。
pub fn save_config(path: &Path, cfg: &AppConfig) -> Result<(), ConfigError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(cfg)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SourceConfig {
        SourceConfig {
            id: "x".into(),
            name: None,
            owner: "gaubee".into(),
            repo: "gaubee.com".into(),
            git_ref: String::new(),
            collection: Collection::Articles,
            include: "src/content/articles/**/*.md".into(),
            exclude: None,
            slug_prefix: None,
            interval: "1h".into(),
            enabled: true,
        }
    }

    #[test]
    fn config_toml_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.github_token = None;
        cfg.sources = vec![sample()];
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(back.sources, cfg.sources);
        assert!(text.contains("[[sources]]"));
    }

    #[test]
    fn derive_id_stable_and_distinct() {
        let a = SourceConfig::derive_id("o", "r", "", "a/**/*.md");
        let b = SourceConfig::derive_id("o", "r", "", "a/**/*.md");
        let c = SourceConfig::derive_id("o", "r", "", "b/**/*.md");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn validate_rejects_bad_glob_and_interval() {
        let mut s = sample();
        assert!(s.validate().is_ok());
        s.include = "[".into();
        assert!(matches!(s.validate(), Err(ValidationError::BadInclude(_))));
        s.include = "a/**".into();
        s.interval = "fast".into();
        assert!(matches!(s.validate(), Err(ValidationError::BadInterval(_))));
    }

    #[test]
    fn interval_parses() {
        assert_eq!(sample().interval_duration(), Duration::from_secs(3600));
    }

    #[test]
    fn site_footer_links_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.site.footer_links = vec![FooterLink {
            id: "beian".into(),
            label: "闽ICP备17026139号-1".into(),
            url: "https://beian.miit.gov.cn/".into(),
        }];
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(text.contains("[[site.footer_links]]"));
        let back: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(back.site.footer_links, cfg.site.footer_links);
        // 无 site 段的旧配置（向后兼容）
        let old_cfg: AppConfig = toml::from_str("sources = []\n").unwrap();
        assert!(old_cfg.site.footer_links.is_empty());
    }

    #[test]
    fn load_missing_file_returns_default() {
        let cfg = load_config(Path::new("/nonexistent/config.toml")).unwrap();
        assert!(cfg.sources.is_empty());
    }
}
