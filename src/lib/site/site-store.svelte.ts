/**
 * 站点展示配置 store（底部状态栏外链 + 站点 SEO 字段）。
 *
 * 事实源在后端 config.toml 的 [site] 段（全站生效、多端一致）：
 * GET/PUT /api/site（见 static-server api.rs）。后端不可达（纯静态预览等）
 * 时回退 SITE 常量默认（GitHub 源码链接）。
 *
 * 典型用法：SystemFooterBar 渲染 footerLinks；seoStore 消费 SEO 字段；
 * 设置「站点」面板编辑并 save()。
 */
import { SITE } from "./site";

export interface FooterLink {
  id: string;
  label: string;
  url: string;
}

/** 站点 SEO 字段（镜像后端 SiteConfig）。 */
export interface SiteSeo {
  site_name: string;
  description?: string | null;
  base_url?: string | null;
  og_image?: string | null;
  allow_indexing: boolean;
}

/** 离线回退默认（与后端出厂默认一致：GitHub 源码链接一条）。 */
const FALLBACK_LINKS: FooterLink[] = [{ id: "github", label: "GitHub", url: SITE.githubUrl }];

const FALLBACK_SEO: SiteSeo = {
  site_name: "GaubeeOS",
  description: null,
  base_url: null,
  og_image: null,
  allow_indexing: true,
};

/** GET /api/site 响应形状。 */
interface SitePayload {
  footer_links?: FooterLink[];
  site_name?: string;
  description?: string | null;
  base_url?: string | null;
  og_image?: string | null;
  allow_indexing?: boolean;
}

class SiteStore {
  /** 底部状态栏外链（响应式；save 后即时反映到 FooterBar）。 */
  footerLinks = $state<FooterLink[]>(FALLBACK_LINKS.map((l) => ({ ...l })));

  /** 站点 SEO 字段（seoStore 消费）。 */
  site = $state<SiteSeo>({ ...FALLBACK_SEO });

  /** 是否已从后端装载（失败时保持回退并记录错误）。 */
  loaded = $state(false);
  error = $state<string | null>(null);

  /** 便捷：生效站点名。 */
  get siteName(): string {
    return this.site.site_name?.trim() || "GaubeeOS";
  }

  /** 便捷：站点默认描述（可为空串）。 */
  get siteDescription(): string {
    return this.site.description ?? "";
  }

  private request(path: string, init?: RequestInit): Promise<unknown> {
    return fetch(`/api${path}`, {
      ...init,
      headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    }).then(async (resp) => {
      const text = await resp.text();
      const data = text ? JSON.parse(text) : undefined;
      if (!resp.ok) {
        const msg =
          data && typeof data === "object" && "error" in data
            ? String((data as { error: unknown }).error)
            : `HTTP ${resp.status}`;
        throw new Error(msg);
      }
      return data;
    });
  }

  private applyPayload(site: SitePayload): void {
    this.footerLinks = (site.footer_links ?? []).map((l) => ({ ...l }));
    this.site = {
      site_name: site.site_name ?? FALLBACK_SEO.site_name,
      description: site.description ?? null,
      base_url: site.base_url ?? null,
      og_image: site.og_image ?? null,
      allow_indexing: site.allow_indexing ?? true,
    };
  }

  /** 从后端装载（SPA boot 时调用；失败静默回退默认）。 */
  async load(): Promise<void> {
    try {
      this.applyPayload((await this.request("/site")) as SitePayload);
      this.loaded = true;
      this.error = null;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  /** 校验并保存完整站点配置（外链 + SEO 字段）到后端；成功后刷新本地状态。 */
  async save(input: { links: FooterLink[]; seo: SiteSeo }): Promise<void> {
    await this.request("/site", {
      method: "PUT",
      body: JSON.stringify({
        footer_links: input.links.map((l) => ({ id: l.id, label: l.label, url: l.url })),
        site_name: input.seo.site_name,
        description: input.seo.description || null,
        base_url: input.seo.base_url || null,
        og_image: input.seo.og_image || null,
        allow_indexing: input.seo.allow_indexing,
      }),
    });
    await this.load();
  }
}

/** 全局单例。 */
export const siteStore = new SiteStore();
