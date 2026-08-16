import { siteStore } from "$lib/site/site-store.svelte";
/**
 * SEO head 管理器（App 级 SEO 的运行时核心，2026-08-16）。
 *
 * 数据流：
 * - SeoRouteBridge（挂 AppShell）：路由切换 → leaf.route.seo（静态声明）→ setSEO
 * - 视图组件（如 ArticleDetailView）：$effect 内 setSEO 覆盖动态值（标题/描述）
 *   （effect 顺序：父 AppShell 先设默认，子组件后覆盖）
 * - SPA 壳 layout 的 <svelte:head> 消费本 store 渲染 title/meta/og/canonical
 *
 * 站点级默认（站点名/描述/base_url/og_image/allow_indexing）来自 siteStore（后端 [site]）。
 * 标题模板：`{title} · {site_name}`；canonical = base_url + pathname（配置时）。
 */
import { untrack } from "svelte";

/** 当前生效的 SEO 状态。 */
export interface SeoState {
  /** 页面标题（不含站点名后缀；空 = 仅站点名）。 */
  title: string;
  description: string;
  noindex: boolean;
  /** og:type。 */
  ogType: "website" | "article";
}

class SeoStore {
  current = $state<SeoState>({
    title: "",
    description: "",
    noindex: false,
    ogType: "website",
  });

  /** 路由切换时由 SeoRouteBridge 调用（设静态默认，清除上一路由的动态覆盖）。 */
  setRouteDefaults(seo: { title?: string; description?: string; noindex?: boolean }): void {
    this.current = {
      title: seo.title ?? "",
      description: seo.description ?? "",
      noindex: seo.noindex ?? false,
      ogType: "website",
    };
  }

  /** 视图组件覆盖动态值（部分字段）。
   * untrack：effect 内调用时读 current 不注册依赖（否则读+写自身 = 无限循环）。 */
  setSEO(override: Partial<SeoState>): void {
    const base = untrack(() => this.current);
    this.current = { ...base, ...override };
  }

  // ---- 消费端派生（<svelte:head> 绑定） ----

  /** 完整标题：`{title} · {site_name}`（无 title 时仅站点名）。 */
  get fullTitle(): string {
    const name = siteStore.siteName;
    return this.current.title ? `${this.current.title} · ${name}` : name;
  }

  /** 生效描述（页面 > 站点默认 > 空）。 */
  get effectiveDescription(): string {
    return this.current.description || siteStore.siteDescription;
  }

  /** robots meta 内容。 */
  get robotsContent(): string {
    return this.current.noindex || !siteStore.site?.allow_indexing
      ? "noindex, nofollow"
      : "index, follow";
  }

  /** canonical URL（base_url 配置时；否则 null）。 */
  get canonical(): string | null {
    const base = siteStore.site?.base_url;
    if (!base) return null;
    const path = typeof location !== "undefined" ? location.pathname + location.search : "/";
    return `${base.replace(/\/+$/, "")}${path}`;
  }
}

/** 全局单例。 */
export const seoStore = new SeoStore();
