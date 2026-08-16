/**
 * 内容源订阅的全局 store（runes）。
 *
 * 职责：
 * 1. 数据装载：sources（订阅列表）+ manifest（聚合清单）+ files（uid → markdown 正文）。
 *    ensureLoaded 单飞（并发调用共享同一 Promise）；正文并发 8 批量拉取。
 * 2. CRUD 联动：新增/更新/同步/启停/删除后自动刷新列表，内容变化时 reloadContent。
 * 3. 派生工具：entriesFor(collection)（content-pipeline 的远程 source 消费）、
 *    bodyOf(uid)、contentUrl(entry)（slugPrefix 前缀路由）、primaryRepo（编辑器跳转）。
 *
 * 状态机：idle → loading → ready | error（后端不可达时不阻塞 OS 启动，视图显示空态）。
 */
import { browser } from "$app/environment";
import { registerDefaultRepo } from "$lib/github/client";

import * as api from "./client";
import type { Manifest, ManifestEntry, SourceInput, SourceWithState } from "./types";

export type ContentSourceStatus = "idle" | "loading" | "ready" | "error";

class ContentSourceStore {
  /** 订阅列表（配置 + 运行态）。 */
  sources = $state<SourceWithState[]>([]);
  /** 聚合清单。 */
  manifest = $state<Manifest | null>(null);
  /** uid → markdown 正文（正文不进 $state 深代理：体积大且只被管道同步读取）。 */
  files = new Map<string, string>();
  status = $state<ContentSourceStatus>("idle");
  error = $state<string | null>(null);

  /** ensureLoaded 单飞锁。 */
  #loading: Promise<void> | null = null;

  /** 确保数据已装载（幂等，浏览器端专用）。 */
  ensureLoaded(): Promise<void> {
    if (!browser) return Promise.resolve();
    if (this.status === "ready" || this.status === "loading")
      return this.#loading ?? Promise.resolve();
    this.#loading = this.#load();
    return this.#loading;
  }

  async #load(): Promise<void> {
    this.status = "loading";
    this.error = null;
    try {
      await Promise.all([this.#refreshSources(), this.#reloadContent()]);
      this.status = "ready";
    } catch (e) {
      this.status = "error";
      this.error = e instanceof Error ? e.message : String(e);
    } finally {
      this.#loading = null;
      this.#syncDefaultRepo();
    }
  }

  /** 把主仓库注入 github/client 的默认 RepoRef（无订阅时置 null，调用方得到显式错误）。 */
  #syncDefaultRepo(): void {
    registerDefaultRepo(this.primaryRepo ? { ...this.primaryRepo } : null);
  }

  async #refreshSources(): Promise<void> {
    this.sources = await api.listSources();
  }

  /** 重新拉 manifest + 全部正文（同步/CRUD 后调用）。 */
  async #reloadContent(): Promise<void> {
    const manifest = await api.getManifest();
    this.manifest = manifest;
    const next = new Map<string, string>();
    const entries = manifest.entries;
    // 并发 8 批量拉正文
    for (let i = 0; i < entries.length; i += 8) {
      const chunk = entries.slice(i, i + 8);
      const bodies = await Promise.all(chunk.map((e) => api.getFile(e.uid).catch(() => null)));
      chunk.forEach((e, j) => {
        if (bodies[j] !== null) next.set(e.uid, bodies[j] as string);
      });
    }
    this.files = next;
  }

  /** 内容变更后的完整刷新（列表 + 清单 + 正文）。保持 status。 */
  async refresh(): Promise<void> {
    if (!browser) return;
    try {
      await Promise.all([this.#refreshSources(), this.#reloadContent()]);
      this.status = "ready";
      this.error = null;
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e);
    }
    this.#syncDefaultRepo();
  }

  // ---- CRUD（成功后自动刷新；失败向上抛给 UI toast）----

  async create(input: SourceInput) {
    const r = await api.createSource(input);
    await this.refresh();
    return r;
  }

  async update(id: string, input: Parameters<typeof api.updateSource>[1]) {
    const r = await api.updateSource(id, input);
    await this.refresh();
    return r;
  }

  async remove(id: string) {
    await api.deleteSource(id);
    await this.refresh();
  }

  async setEnabled(id: string, enabled: boolean) {
    await api.setEnabled(id, enabled);
    await this.#refreshSources();
    this.#syncDefaultRepo();
  }

  async syncNow(id: string) {
    const outcome = await api.syncSource(id);
    await this.refresh();
    return outcome;
  }

  // ---- 派生工具 ----

  /** 某集合的清单条目（date 降序尽力而为，最终排序由查询层做）。 */
  entriesFor(collection: string): ManifestEntry[] {
    return (this.manifest?.entries ?? []).filter((e) => e.collection === collection);
  }

  /** 取正文（未装载返回 null）。 */
  bodyOf(uid: string): string | null {
    return this.files.get(uid) ?? null;
  }

  /** URL slug：应用 slug_prefix 前缀（多源防冲突）。 */
  slugOf(entry: { uid: string; slug: string; slug_prefix: string }): string {
    return `${entry.slug_prefix ?? ""}${entry.slug}`;
  }

  /** 详情页 URL（/article/{collection}/{prefix?}{slug}）。 */
  contentUrl(entry: {
    uid: string;
    collection: string;
    slug: string;
    slug_prefix: string;
  }): string {
    return `/article/${entry.collection}/${this.slugOf(entry)}`;
  }

  /** 主仓库（编辑器跳转/新建内容目标）。 */
  get primaryRepo(): { owner: string; repo: string; ref: string } | null {
    return api.primaryRepoOf(this.sources);
  }

  /** 编辑器跳转链接（无主仓库返回 null，调用方隐藏入口）。 */
  editorHrefFor(path: string): string | null {
    return api.editorHrefFor(this.sources, path);
  }
}

/** 全局单例。 */
export const contentSourceStore = new ContentSourceStore();
