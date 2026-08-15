//! 内容清单（manifest）与最小 frontmatter 抽取。
//!
//! 产物：`{data_dir}/cache/manifest.json`（全局聚合，前端唯一消费入口）。
//! frontmatter 只做「列表页够用」的最小抽取（title/date/updated/tags），
//! 完整解析（自定义字段、正文渲染）仍在前端 parseMarkdown——单一职责切割。

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::SourceConfig;

/// 清单版本（结构变更时递增，前端按版本兼容）。
pub const MANIFEST_VERSION: u32 = 1;

/// 源归属（清单内嵌，前端跳转编辑器/展示用）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceRef {
    pub id: String,
    pub owner: String,
    pub repo: String,
    /// 解析后的 ref（分支名或 sha，空 ref 同步后为默认分支名）。
    #[serde(rename = "ref")]
    pub git_ref: String,
}

/// 单条内容条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestEntry {
    /// 全局唯一：`{sourceId}:{仓库相对路径}`。
    pub uid: String,
    pub source: SourceRef,
    pub collection: String,
    /// 仓库相对路径（含文件名）。
    pub path: String,
    pub filename: String,
    /// URL slug：filename 去扩展名（前端拼 `{slugPrefix}{slug}` 成路由段）。
    pub slug: String,
    pub slug_prefix: String,
    pub title: Option<String>,
    /// ISO 日期字符串（原样透传 frontmatter，排序由前端解析）。
    pub date: Option<String>,
    pub updated: Option<String>,
    pub tags: Vec<String>,
    pub bytes: u64,
    pub synced_at: String,
}

/// 全局清单。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub generated_at: String,
    pub entries: Vec<ManifestEntry>,
}

/// frontmatter 最小抽取结果。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FrontmatterMeta {
    pub title: Option<String>,
    pub date: Option<String>,
    pub updated: Option<String>,
    pub tags: Vec<String>,
}

/// 从 markdown 抽取最小 frontmatter（容忍各种写法，抽取不到就留 None）。
///
/// 支持：
/// - `title: 单行标量`（含单/双引号包裹，去引号）
/// - `date:` / `updated:` ISO 字符串
/// - `tags: [a, b]`（行内数组）或 `tags:` 后跟若干 `  - item`（块数组）
pub fn extract_frontmatter(md: &str) -> FrontmatterMeta {
    let mut meta = FrontmatterMeta::default();
    let mut lines = md.lines();
    if lines.next().map(str::trim) != Some("---") {
        return meta;
    }
    let mut in_tags_block = false;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed.trim() == "---" {
            break;
        }
        if in_tags_block {
            let t = trimmed.trim_start();
            if let Some(item) = t.strip_prefix("- ") {
                meta.tags.push(unquote(item.trim()));
                continue;
            }
            in_tags_block = false; // 非 `- ` 行，块数组结束，回落到键值解析
        }
        let Some((key, value)) = split_kv(trimmed) else {
            continue;
        };
        match key {
            "title" => meta.title = Some(unquote(value.trim())),
            "date" => meta.date = Some(unquote(value.trim())),
            "updated" => meta.updated = Some(unquote(value.trim())),
            "tags" => {
                let v = value.trim();
                if let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                    meta.tags = inner
                        .split(',')
                        .map(|s| unquote(s.trim()))
                        .filter(|s| !s.is_empty())
                        .collect();
                } else if v.is_empty() {
                    // 块数组：tags: 后续的 `- item` 行
                    in_tags_block = true;
                }
            }
            _ => {}
        }
    }
    meta
}

/// `key: value` 切分（无值 → None；`key:` 空值返回 Some((key, ""))）。
fn split_kv(line: &str) -> Option<(&str, &str)> {
    let idx = line.find(':')?;
    let key = line[..idx].trim();
    if key.is_empty() || key.starts_with('#') {
        return None;
    }
    Some((key, &line[idx + 1..]))
}

/// 去掉成对的单/双引号。
fn unquote(s: &str) -> String {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

/// 由源配置 + 仓库路径 + 文件内容构建清单条目。
pub fn build_entry(
    cfg: &SourceConfig,
    resolved_ref: &str,
    path: &str,
    content: &str,
    synced_at: &str,
) -> ManifestEntry {
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    let slug = filename
        .strip_suffix(".md")
        .unwrap_or(&filename)
        .to_string();
    let meta = extract_frontmatter(content);
    ManifestEntry {
        uid: format!("{}:{path}", cfg.id),
        source: SourceRef {
            id: cfg.id.clone(),
            owner: cfg.owner.clone(),
            repo: cfg.repo.clone(),
            git_ref: resolved_ref.to_string(),
        },
        collection: cfg.collection.as_str().to_string(),
        path: path.to_string(),
        filename,
        slug,
        slug_prefix: cfg.slug_prefix.clone().unwrap_or_default(),
        title: meta.title,
        date: meta.date,
        updated: meta.updated,
        tags: meta.tags,
        bytes: content.len() as u64,
        synced_at: synced_at.to_string(),
    }
}

/// 聚合多源条目 → 全局清单（按 collection 稳定分组，各自 date 降序尽力而为：
/// 日期字符串排序可能因格式差异不完全准确，最终排序由前端执行）。
pub fn aggregate(entries: Vec<ManifestEntry>) -> Manifest {
    let mut entries = entries;
    entries.sort_by(|a, b| {
        a.collection
            .cmp(&b.collection)
            .then(b.date.cmp(&a.date))
            .then(a.uid.cmp(&b.uid))
    });
    Manifest {
        version: MANIFEST_VERSION,
        generated_at: chrono::Utc::now().to_rfc3339(),
        entries,
    }
}

/// 原子写 manifest.json。
pub fn save_manifest(path: &Path, manifest: &Manifest) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(manifest).unwrap_or_default(),
    )?;
    std::fs::rename(&tmp, path)
}

/// 聚合各源的条目表（id → entries）→ 写盘。sync 每轮结束后调用。
pub fn rebuild_manifest(
    data_dir: &Path,
    per_source: &HashMap<String, Vec<ManifestEntry>>,
) -> std::io::Result<()> {
    let all: Vec<ManifestEntry> = per_source.values().flatten().cloned().collect();
    let m = aggregate(all);
    save_manifest(&data_dir.join("cache").join("manifest.json"), &m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Collection;

    const MD_INLINE: &str =
        "---\ntitle: \"Hello World\"\ndate: 2025-04-09T08:00:00Z\ntags: [rust, axum]\n---\n\n正文";
    const MD_BLOCK: &str =
        "---\ntitle: 块数组\nupdated: '2026-01-02'\ntags:\n  - a\n  - \"b c\"\n---\nbody";
    const MD_NONE: &str = "# 没有前置元数据";

    #[test]
    fn extracts_inline_frontmatter() {
        let m = extract_frontmatter(MD_INLINE);
        assert_eq!(m.title.as_deref(), Some("Hello World"));
        assert_eq!(m.date.as_deref(), Some("2025-04-09T08:00:00Z"));
        assert_eq!(m.tags, vec!["rust", "axum"]);
        assert!(m.updated.is_none());
    }

    #[test]
    fn extracts_block_tags_and_quotes() {
        let m = extract_frontmatter(MD_BLOCK);
        assert_eq!(m.title.as_deref(), Some("块数组"));
        assert_eq!(m.updated.as_deref(), Some("2026-01-02"));
        assert_eq!(m.tags, vec!["a", "b c"]);
    }

    #[test]
    fn no_frontmatter_is_empty() {
        assert_eq!(extract_frontmatter(MD_NONE), FrontmatterMeta::default());
    }

    #[test]
    fn builds_entry_with_uid_and_slug() {
        let cfg = SourceConfig {
            id: "s1".into(),
            name: None,
            owner: "o".into(),
            repo: "r".into(),
            git_ref: String::new(),
            collection: Collection::Articles,
            include: "**/*.md".into(),
            exclude: None,
            slug_prefix: Some("blog-".into()),
            interval: "1h".into(),
            enabled: true,
        };
        let e = build_entry(
            &cfg,
            "main",
            "src/content/articles/0063.demo.md",
            MD_INLINE,
            "t0",
        );
        assert_eq!(e.uid, "s1:src/content/articles/0063.demo.md");
        assert_eq!(e.slug, "0063.demo");
        assert_eq!(e.slug_prefix, "blog-");
        assert_eq!(e.collection, "articles");
        assert_eq!(e.title.as_deref(), Some("Hello World"));
        assert_eq!(e.bytes, MD_INLINE.len() as u64);
    }

    #[test]
    fn aggregate_sorts_by_collection_then_date_desc() {
        let mk = |uid: &str, coll: &str, date: Option<&str>| ManifestEntry {
            uid: uid.into(),
            source: SourceRef {
                id: "s".into(),
                owner: "o".into(),
                repo: "r".into(),
                git_ref: "main".into(),
            },
            collection: coll.into(),
            path: format!("{uid}.md"),
            filename: format!("{uid}.md"),
            slug: uid.into(),
            slug_prefix: String::new(),
            title: None,
            date: date.map(str::to_string),
            updated: None,
            tags: vec![],
            bytes: 1,
            synced_at: String::new(),
        };
        let m = aggregate(vec![
            mk("a", "articles", Some("2025-01-01")),
            mk("b", "events", Some("2026-01-01")),
            mk("c", "articles", Some("2026-03-03")),
            mk("d", "articles", None),
        ]);
        let uids: Vec<&str> = m.entries.iter().map(|e| e.uid.as_str()).collect();
        // articles 组在前（date 降序，None 排最后），events 组在后
        assert_eq!(uids, vec!["c", "a", "d", "b"]);
    }
}
