import { contentQuery } from "$lib/content-pipeline/query.svelte";
import type { ContentEntry } from "$lib/content-pipeline/types";
/**
 * 运行时搜索服务（内核订阅模式，2026-08-16）。
 *
 * 与 file-service（静态分片，gaubee.com 单体模式）对偶：从订阅引擎装载的
 * ContentEntry[] 在内存建 MiniSearch 索引，单批返回全部命中。
 *
 * 投影与 entryToSearchDocument 同构，唯 href 改用 contentQuery.contentUrl
 * （远程条目 path 无 src/content/{collection}/ 前缀，且需应用 slug_prefix）。
 */
import MiniSearch from "minisearch";

import { miniSearchOptions, type SearchIndexDocument } from "./minisearch";
import type { SearchBatch, SearchResult, SearchService, SearchTask } from "./types";

interface RuntimeSearchServiceOptions {
  /** 搜索服务标识（注册表 key，如 'articles' / 'shout'）。 */
  appId: string;
  /** 内容集合（管道 entries 键）：articles | events。 */
  collection: "articles" | "events";
  appName: string;
}

function entryToRuntimeDocument(entry: ContentEntry): SearchIndexDocument {
  return {
    id: entry.uid,
    title: entry.title,
    content: entry.body,
    tags: entry.tags.join(" "),
    excerpt: entry.excerpt,
    href: contentQuery.contentUrl(entry),
    date: entry.date.getTime(),
  };
}

/** 索引缓存：内容版本变化（contentQuery.version）时重建。 */
let cachedVersion = -1;
let cachedAppId = "";
let cachedIndex: MiniSearch<SearchIndexDocument> | null = null;

function getIndex(
  appId: string,
  collection: "articles" | "events",
): MiniSearch<SearchIndexDocument> {
  if (cachedIndex && cachedVersion === contentQuery.version && cachedAppId === appId) {
    return cachedIndex;
  }
  const entries =
    collection === "articles" ? contentQuery.listArticles() : contentQuery.listEvents();
  const ms = new MiniSearch<SearchIndexDocument>(miniSearchOptions);
  ms.addAll(entries.map(entryToRuntimeDocument));
  cachedVersion = contentQuery.version;
  cachedAppId = appId;
  cachedIndex = ms;
  return ms;
}

function toSearchResult(
  appId: string,
  appName: string,
  hit: { id: string | number; score: number } & SearchIndexDocument,
): SearchResult {
  return {
    id: `${appId}:${String(hit.id)}`,
    appId,
    appName,
    title: hit.title,
    excerpt: hit.excerpt,
    href: hit.href,
    date: hit.date,
    score: hit.score,
  };
}

/** 为订阅内容应用创建运行时搜索服务（manifest.searchService 注册）。 */
export function createRuntimeSearchService(options: RuntimeSearchServiceOptions): SearchService {
  return {
    appId: options.appId,
    appName: options.appName,
    async *search(task: SearchTask): AsyncIterable<SearchBatch> {
      await contentQuery.init();
      if (task.signal.aborted) return;
      const ms = getIndex(options.appId, options.collection);
      if (task.signal.aborted) return;
      const hits = ms.search(task.query.engineQuery ?? MiniSearch.wildcard, {
        prefix: true,
        fuzzy: 0.2,
      });
      const results = hits
        .map((hit) => toSearchResult(options.appId, options.appName, hit as never))
        .sort((left, right) => right.date - left.date || right.score - left.score);
      yield { appId: options.appId, appName: options.appName, results, complete: true };
    },
  };
}
