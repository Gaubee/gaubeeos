/**
 * 统一内容查询层（单例）。
 *
 * 替代 readonlyVfs.getPosts + contentStore 派生视图，作为所有消费方的唯一入口。
 *
 * 数据源策略（内核订阅模式，2026-08-16）：
 * - init()：异步 —— 先装载订阅数据（/api/content/*，见 content-source store），
 *   再跑管道（远程 source 忽略 reader，直接读 store）。幂等 + 单飞。
 * - refresh()：手动重跑管道（订阅 CRUD/同步后由 store 联动调用方触发）。
 *
 * SSR 安全：管道只在 browser 执行。
 * 响应式：通过 version 计数器暴露，消费方可用 contentQuery.version 触发 $derived 重算。
 */
import { browser } from "$app/environment";
import { contentSourceStore } from "$lib/content-source/store.svelte";

import { pipelineExecutor } from "./executor";
import type { ContentEntry, VfsReader } from "./types";

/** 标签统计项。 */
export interface TagCount {
  tag: string;
  count: number;
}

// ---- reader 适配器 ----

/** 占位 reader：远程模式下 ContentSource.read 忽略 reader（见 sources/remote.ts）。 */
const nullReader: VfsReader = {
  readFile: () => null,
  readdir: () => [],
};

/** 按 date 降序排序的复用工具。 */
function byDateDesc(a: ContentEntry, b: ContentEntry): number {
  return b.date.getTime() - a.date.getTime();
}

class ContentQuery {
  /** 管道执行版本（每次 init/refresh 自增，供消费方响应式追踪）。 */
  version = $state(0);

  /** 是否已初始化（至少跑过一次管道）。 */
  initialized = $state(false);

  /** 订阅数据装载中（视图可用它显示骨架屏）。 */
  loading = $state(false);

  /** 装载错误（后端不可达等，视图显示错误条）。 */
  error = $state<string | null>(null);

  #initPromise: Promise<void> | null = null;

  /** 初始化管道（异步：装载订阅数据 → 跑管道）。幂等 + 单飞。 */
  init(): Promise<void> {
    if (!browser) return Promise.resolve();
    if (!this.#initPromise) {
      this.loading = true;
      this.error = null;
      this.#initPromise = (async () => {
        await contentSourceStore.ensureLoaded();
        pipelineExecutor.run(nullReader);
        this.version++;
        this.initialized = true;
        this.error = contentSourceStore.error;
      })()
        .catch((e) => {
          this.error = e instanceof Error ? e.message : String(e);
        })
        .finally(() => {
          this.loading = false;
          this.#initPromise = null;
        });
    }
    return this.#initPromise;
  }

  /** 重新执行管道（订阅 CRUD/同步后调用；数据已在 store 刷新）。 */
  refresh(): void {
    if (!browser) return;
    pipelineExecutor.run(nullReader);
    this.version++;
    this.initialized = true;
    this.error = contentSourceStore.error;
  }

  /** 订阅数据变化后的完整刷新（store.refresh 完成后调用）。 */
  async refreshFromRemote(): Promise<void> {
    if (!browser) return;
    await contentSourceStore.ensureLoaded();
    await contentSourceStore.refresh();
    this.refresh();
  }

  // ---- 查询 API ----

  /** 所有文章（按 date 降序）。 */
  listArticles(opts?: { limit?: number }): ContentEntry[] {
    const entries = pipelineExecutor.getEntries("articles").slice().sort(byDateDesc);
    return opts?.limit ? entries.slice(0, opts.limit) : entries;
  }

  /** 所有说说/事件（按 date 降序）。 */
  listEvents(opts?: { limit?: number }): ContentEntry[] {
    const entries = pipelineExecutor.getEntries("events").slice().sort(byDateDesc);
    return opts?.limit ? entries.slice(0, opts.limit) : entries;
  }

  /** 全部内容合并（按 date 降序）。 */
  allPosts(): ContentEntry[] {
    return pipelineExecutor.getAllEntries().slice().sort(byDateDesc);
  }

  /** 标签频次（降序）。来自 tags processor 缓存，未注册时降级到本地计算。 */
  listTags(): TagCount[] {
    const result = pipelineExecutor.getResult("tags");
    if (result?.type === "tags") {
      return result.data as TagCount[];
    }
    // 降级：直接遍历全量 entries 统计
    return this.computeTags(pipelineExecutor.getAllEntries());
  }

  /** 带指定标签的所有内容（按 date 降序）。 */
  byTag(tag: string): ContentEntry[] {
    return this.allPosts().filter((e) => e.tags.includes(tag));
  }

  /** 按年份分组（年份降序）。 */
  groupByYear(entries: ContentEntry[]): Map<number, ContentEntry[]> {
    const groups = new Map<number, ContentEntry[]>();
    for (const entry of entries) {
      const year = entry.date.getFullYear();
      if (!groups.has(year)) groups.set(year, []);
      groups.get(year)!.push(entry);
    }
    return new Map([...groups.entries()].sort((a, b) => b[0] - a[0]));
  }

  /**
   * 根据 collection + URL slug 查找单篇。
   * 匹配 stem 本身或 slug_prefix 前缀形式（多源防冲突，见 contentUrl）。
   */
  findPost(collection: string, stem: string): ContentEntry | null {
    const entries = pipelineExecutor.getEntries(collection);
    return (
      entries.find((e) => this.urlSlugOf(e) === stem) ??
      entries.find((e) => e.id.stem === stem) ??
      null
    );
  }

  /** 同集合的所有内容（按 date 降序，供上下篇导航）。 */
  siblings(collection: string): ContentEntry[] {
    return pipelineExecutor.getEntries(collection).slice().sort(byDateDesc);
  }

  /** 统一 excerpt 查询（供消费方按需取摘要）。 */
  excerptFor(entry: ContentEntry): string {
    return entry.excerpt;
  }

  /** entry 的 URL slug（应用其源配置的 slug_prefix）。 */
  urlSlugOf(entry: ContentEntry): string {
    const me = contentSourceStore.manifest?.entries.find((e) => e.uid === entry.uid);
    return me ? `${me.slug_prefix ?? ""}${entry.id.stem}` : entry.id.stem;
  }

  /** entry 详情页 URL。 */
  contentUrl(entry: ContentEntry): string {
    return `/article/${entry.collection}/${this.urlSlugOf(entry)}`;
  }

  // ---- 内部工具 ----

  private computeTags(entries: ContentEntry[]): TagCount[] {
    const counts = new Map<string, number>();
    for (const entry of entries) {
      for (const tag of entry.tags) {
        counts.set(tag, (counts.get(tag) ?? 0) + 1);
      }
    }
    return [...counts.entries()]
      .map(([tag, count]) => ({ tag, count }))
      .sort((a, b) => b.count - a.count);
  }
}

/** 全局单例。 */
export const contentQuery = new ContentQuery();
