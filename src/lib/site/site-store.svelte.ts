/**
 * 站点展示配置 store（底部状态栏外链）。
 *
 * 事实源在后端 config.toml 的 [site] 段（全站生效、多端一致）：
 * GET/PUT /api/site（见 static-server api.rs）。后端不可达（纯静态预览等）
 * 时回退 SITE 常量默认（GitHub 源码链接）。
 *
 * 典型用法：SystemFooterBar 渲染 footerLinks；设置「状态栏」面板编辑并 save()。
 */
import { SITE } from "./site";

export interface FooterLink {
  id: string;
  label: string;
  url: string;
}

/** 离线回退默认（与后端出厂默认一致：GitHub 源码链接一条）。 */
const FALLBACK_LINKS: FooterLink[] = [{ id: "github", label: "GitHub", url: SITE.githubUrl }];

class SiteStore {
  /** 底部状态栏外链（响应式；save 后即时反映到 FooterBar）。 */
  footerLinks = $state<FooterLink[]>(FALLBACK_LINKS.map((l) => ({ ...l })));

  /** 是否已从后端装载（失败时保持回退并记录错误）。 */
  loaded = $state(false);
  error = $state<string | null>(null);

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

  /** 从后端装载（SPA boot 时调用；失败静默回退默认）。 */
  async load(): Promise<void> {
    try {
      const site = (await this.request("/site")) as { footer_links?: FooterLink[] };
      this.footerLinks = (site.footer_links ?? []).map((l) => ({ ...l }));
      this.loaded = true;
      this.error = null;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
  }

  /** 校验并保存到后端；成功后更新本地状态（FooterBar 即时生效）。 */
  async save(links: FooterLink[]): Promise<void> {
    await this.request("/site", {
      method: "PUT",
      body: JSON.stringify({
        footer_links: links.map((l) => ({ id: l.id, label: l.label, url: l.url })),
      }),
    });
    this.footerLinks = links.map((l) => ({ ...l }));
    this.loaded = true;
    this.error = null;
    await this.load();
  }
}

/** 全局单例。 */
export const siteStore = new SiteStore();
