//! SEO 产物生成（OS 级）：robots.txt / sitemap.xml / 文章 meta shell。
//!
//! 设计（2026-08-16）：
//! - 零 UA 嗅探：文章 meta shell 是「SPA 壳的变换」——人类用户与爬虫拿到同一 HTML，
//!   爬虫读 head meta + JSON-LD + noscript 文本，社交预览（微信/TG/Slack）直接可用；
//!   人类用户 SPA 正常启动，无体验差异。
//! - 数据源：SiteConfig（[site] 段）+ AppState runtime 内的内容清单 + 正文缓存。

use crate::config::SiteConfig;
use crate::manifest::ManifestEntry;

/// HTML 文本转义（meta/noscript 注入防 XSS）。
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// XML 文本转义（sitemap）。
pub fn xml_escape(s: &str) -> String {
    html_escape(s)
}

/// robots.txt 生成。
pub fn render_robots(site: &SiteConfig) -> String {
    let mut out = String::new();
    out.push_str("User-agent: *\n");
    if site.allow_indexing {
        out.push_str("Disallow:\n");
    } else {
        out.push_str("Disallow: /\n");
    }
    if let Some(base) = site.base_url.as_deref().filter(|b| !b.trim().is_empty()) {
        if site.allow_indexing {
            out.push_str(&format!("\nSitemap: {base}/sitemap.xml\n"));
        }
    }
    out
}

/// sitemap.xml：从清单条目生成文章 URL；base_url 未配置返回 None。
pub fn render_sitemap(site: &SiteConfig, entries: &[ManifestEntry]) -> Option<String> {
    let base = site
        .base_url
        .as_deref()?
        .trim()
        .trim_end_matches('/')
        .to_string();
    let mut urls = String::new();
    // 首页
    urls.push_str(&format!(
        "  <url>\n    <loc>{}</loc>\n  </url>\n",
        xml_escape(&base)
    ));
    for e in entries {
        let slug = format!("{}{}", e.slug_prefix, e.slug);
        let loc = format!("{}/article/{}/{}", base, e.collection, slug);
        let lastmod = e.updated.as_deref().or(e.date.as_deref()).unwrap_or("");
        urls.push_str(&format!(
            "  <url>\n    <loc>{}</loc>\n{}\n  </url>\n",
            xml_escape(&loc),
            if lastmod.is_empty() {
                String::new()
            } else {
                format!("    <lastmod>{}</lastmod>", xml_escape(lastmod))
            }
        ));
    }
    Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{urls}</urlset>\n"
    ))
}

/// 文章 meta shell：SPA index.html 变换（head 注入 meta 块 + body 注入 noscript）。
///
/// 注入点：`</head>` 前（title 替换 + meta/og/canonical/JSON-LD）、`<body …>` 标签后（noscript）。
pub fn render_article_shell(
    index_html: &str,
    url_slug: &str,
    e: &ManifestEntry,
    body_md: &str,
    site: &SiteConfig,
) -> String {
    let title = e.title.as_deref().unwrap_or(&e.slug);
    let full_title = format!("{} · {}", title, site.name());
    let desc = e
        .description
        .clone()
        .unwrap_or_else(|| crate::manifest::excerpt_from_body(body_md));
    let slug = format!("{}{}", e.slug_prefix, e.slug);
    let canonical = site
        .base_url
        .as_deref()
        .filter(|b| !b.trim().is_empty())
        .map(|b| {
            format!(
                "{}/article/{}/{}",
                b.trim_end_matches('/'),
                e.collection,
                slug
            )
        });
    let og_image = site.og_image.as_deref().filter(|u| !u.trim().is_empty());

    let mut head = String::new();
    head.push_str(&format!("<title>{}</title>", html_escape(&full_title)));
    head.push_str(&format!(
        "<meta name=\"description\" content=\"{}\" />",
        html_escape(&desc)
    ));
    if !site.allow_indexing {
        head.push_str("<meta name=\"robots\" content=\"noindex, nofollow\" />");
    }
    head.push_str(&format!(
        "<meta property=\"og:type\" content=\"article\" /><meta property=\"og:title\" content=\"{}\" />",
        html_escape(&full_title)
    ));
    head.push_str(&format!(
        "<meta property=\"og:description\" content=\"{}\" />",
        html_escape(&desc)
    ));
    if let Some(c) = &canonical {
        head.push_str(&format!(
            "<link rel=\"canonical\" href=\"{}\" /><meta property=\"og:url\" content=\"{}\" />",
            html_escape(c),
            html_escape(c)
        ));
    }
    if let Some(img) = og_image {
        head.push_str(&format!(
            "<meta property=\"og:image\" content=\"{}\" />",
            html_escape(img)
        ));
    }
    head.push_str(&format!(
        "<meta property=\"og:site_name\" content=\"{}\" />",
        html_escape(site.name())
    ));
    // JSON-LD（BlogPosting）
    let jsonld = serde_json::json!({
        "@context": "https://schema.org",
        "@type": "BlogPosting",
        "headline": title,
        "description": desc,
        "datePublished": e.date.as_deref().unwrap_or(""),
        "dateModified": e.updated.as_deref().or(e.date.as_deref()).unwrap_or(""),
        "keywords": e.tags.join(", "),
        "url": canonical.clone().unwrap_or_default(),
    });
    head.push_str(&format!(
        "<script type=\"application/ld+json\">{}</script>",
        html_escape(&jsonld.to_string())
    ));

    // noscript 静态内容（不渲染 JS 的爬虫）
    let noscript = format!(
        "<noscript><article><h1>{}</h1><p>{} · {}</p><p>{}</p><pre>{}</pre></article></noscript>",
        html_escape(title),
        html_escape(e.date.as_deref().unwrap_or("")),
        html_escape(&e.tags.join(", ")),
        html_escape(&desc),
        html_escape(body_md),
    );
    let _ = url_slug;

    // 注入
    let mut html = index_html.to_string();
    // 1. 移除既有 <title>（SPA 壳只有 svelte:head 占位，但 index.html 可能内联 title）
    if let Some(start) = html.find("<title>") {
        if let Some(end) = html.find("</title>") {
            if end > start {
                html.replace_range(start..end + "</title>".len(), "");
            }
        }
    }
    if let Some(pos) = html.find("</head>") {
        html.insert_str(pos, &head);
    }
    if let Some(pos) = html.find("<body") {
        if let Some(gt) = html[pos..].find('>') {
            let insert_at = pos + gt + 1;
            html.insert_str(insert_at, &noscript);
        }
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(allow: bool, base: Option<&str>) -> SiteConfig {
        SiteConfig {
            footer_links: vec![],
            site_name: "MyOS".into(),
            description: None,
            base_url: base.map(str::to_string),
            og_image: None,
            allow_indexing: allow,
        }
    }

    fn entry() -> ManifestEntry {
        ManifestEntry {
            slug: "0063.demo".into(),
            slug_prefix: String::new(),
            collection: "articles".into(),
            title: Some("演示 <标题> & 注入".into()),
            date: Some("2026-08-15T12:00:00Z".into()),
            updated: None,
            tags: vec!["rust".into(), "web".into()],
            description: Some("显式描述 \"引号\" <b>标签</b>".into()),
            uid: "s1:x.md".into(),
            source: crate::manifest::SourceRef {
                id: "s1".into(),
                owner: "o".into(),
                repo: "r".into(),
                git_ref: "main".into(),
            },
            path: "x.md".into(),
            filename: "x.md".into(),
            bytes: 1,
            synced_at: String::new(),
        }
    }

    #[test]
    fn robots_variants() {
        assert!(render_robots(&site(true, None)).contains("Disallow:\n"));
        let disallow = render_robots(&site(false, None));
        assert!(disallow.contains("Disallow: /\n"));
        assert!(!disallow.contains("Sitemap"));
        let with_base = render_robots(&site(true, Some("https://x.com")));
        assert!(with_base.contains("Sitemap: https://x.com/sitemap.xml"));
    }

    #[test]
    fn sitemap_requires_base_url() {
        assert!(render_sitemap(&site(true, None), &[]).is_none());
        let xml = render_sitemap(&site(true, Some("https://x.com/")), &[entry()]).unwrap();
        assert!(xml.contains("https://x.com/article/articles/0063.demo"));
        assert!(xml.contains("<lastmod>2026-08-15T12:00:00Z</lastmod>"));
        assert!(xml.starts_with("<?xml"));
    }

    #[test]
    fn article_shell_escapes_and_injects() {
        let index = "<html><head><title>OLD</title></head><body><div id=app></div></body></html>";
        let html = render_article_shell(
            index,
            "0063.demo",
            &entry(),
            "# 正文 <b>",
            &site(true, Some("https://x.com")),
        );
        assert!(html.contains("演示 &lt;标题&gt; &amp; 注入 · MyOS</title>"));
        assert!(!html.contains("<title>OLD"));
        assert!(html.contains("og:type\" content=\"article"));
        assert!(html.contains("canonical\" href=\"https://x.com/article/articles/0063.demo"));
        assert!(html.contains("BlogPosting"));
        // XSS 转义：描述里的引号/标签
        assert!(html.contains("&quot;引号&quot;"));
        assert!(!html.contains("<b>标签</b>"));
        assert!(html.contains("<noscript>"));
    }

    #[test]
    fn noindex_when_disallowed() {
        let index = "<html><head></head><body></body></html>";
        let html = render_article_shell(index, "x", &entry(), "", &site(false, None));
        assert!(html.contains("noindex, nofollow"));
        assert!(!html.contains("canonical"));
    }
}
