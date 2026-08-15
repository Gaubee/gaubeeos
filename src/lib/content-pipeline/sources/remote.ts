import { contentSourceStore } from "$lib/content-source/store.svelte";
import type { ManifestEntry } from "$lib/content-source/types";
/**
 * 远程内容源：把订阅引擎的清单条目 + 正文装载为 ContentEntry[]。
 *
 * GaubeeOS 内核模式（2026-08-16）：数据来自后端订阅引擎（/api/content/*），
 * 不再读本机 VFS。read(vfs) 的 vfs 参数被忽略（接口保持 ContentSource 形状，
 * 以复用 pipelineExecutor / processors / contentQuery 全套框架）。
 *
 * 兼容性：正文仍走 parseMarkdown（完整 metadata + passthrough），
 * manifest 的最小 frontmatter 仅用于后端排序，前端不依赖。
 */
import { parseArticleId, parseMarkdown } from "$lib/data/frontmatter";

import { createExcerpt } from "../excerpt";
import type { ContentEntry, ContentSource } from "../types";

const FALLBACK_DATE = new Date(0);

/** 单条远程条目 → ContentEntry（正文缺失时跳过，返回 null）。 */
export function entryFromRemote(me: ManifestEntry): ContentEntry | null {
  const raw = contentSourceStore.bodyOf(me.uid);
  if (raw === null) return null;

  const { metadata, body } = parseMarkdown(raw);
  const meta = metadata ?? { date: FALLBACK_DATE, tags: [] };
  const articleId = parseArticleId(me.filename);

  return {
    uid: me.uid,
    path: me.path,
    collection: me.collection,
    filename: me.filename,
    id: articleId,
    title: meta.title ?? articleId.slug ?? articleId.stem,
    date: meta.date,
    updated: meta.updated,
    tags: meta.tags,
    body,
    excerpt: createExcerpt(body),
    metadata: meta,
  };
}

/** 构造某集合的远程 ContentSource（articles/events 共用）。 */
export function remoteCollectionSource(collection: string): ContentSource {
  return {
    collection,
    pathPrefix: "", // 远程模式无本地路径前缀（匹配在后端 glob 完成）
    read() {
      const out: ContentEntry[] = [];
      for (const me of contentSourceStore.entriesFor(collection)) {
        const entry = entryFromRemote(me);
        if (entry) out.push(entry);
      }
      return out;
    },
  };
}
