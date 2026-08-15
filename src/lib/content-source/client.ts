/**
 * 内容源订阅 REST 客户端（同源 /api/*，dev 下经 vite proxy → :8090）。
 *
 * 约定：所有失败抛 Error（message 为后端 {error} 字段），调用方自行 toast。
 */
import type {
  Manifest,
  SourceConfig,
  SourceInput,
  SourceWithState,
  SyncOutcome,
  TestResult,
} from "./types";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const resp = await fetch(`/api${path}`, {
    ...init,
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
  });
  if (resp.status === 204) return undefined as T;
  const text = await resp.text();
  const data = text ? (JSON.parse(text) as unknown) : undefined;
  if (!resp.ok) {
    const msg =
      data && typeof data === "object" && "error" in data
        ? String((data as { error: unknown }).error)
        : `HTTP ${resp.status}`;
    throw new Error(msg);
  }
  return data as T;
}

/** 订阅列表（配置 + 运行态）。 */
export function listSources(): Promise<SourceWithState[]> {
  return request<{ sources: SourceWithState[] }>("/sources").then((r) => r.sources);
}

/** 新增订阅（后端会立即做首轮同步并返回结果）。 */
export function createSource(
  input: SourceInput,
): Promise<{ id: string; outcome: SyncOutcome | null }> {
  return request("/sources", { method: "POST", body: JSON.stringify(input) });
}

/** 全量更新订阅（后端会强制重同步）。 */
export function updateSource(
  id: string,
  input: SourceInput,
): Promise<{ id: string; resynced: boolean }> {
  return request(`/sources/${encodeURIComponent(id)}`, {
    method: "PUT",
    body: JSON.stringify(input),
  });
}

/** 删除订阅（含缓存清理）。 */
export function deleteSource(id: string): Promise<void> {
  return request(`/sources/${encodeURIComponent(id)}`, { method: "DELETE" });
}

/** 启停订阅（停用保留内容，仅停止同步调度）。 */
export function setEnabled(id: string, enabled: boolean): Promise<void> {
  return request(`/sources/${encodeURIComponent(id)}/enabled`, {
    method: "POST",
    body: JSON.stringify({ enabled }),
  });
}

/** 手动同步（等待完成，返回结果）。 */
export function syncSource(id: string): Promise<SyncOutcome> {
  return request(`/sources/${encodeURIComponent(id)}/sync`, { method: "POST" });
}

/** 测试连接：head + glob 命中预览（不下载）。 */
export function testConnection(input: {
  owner: string;
  repo: string;
  ref?: string;
  include: string;
  exclude?: string;
}): Promise<TestResult> {
  return request("/sources/test", { method: "POST", body: JSON.stringify(input) });
}

/** 聚合内容清单。 */
export function getManifest(): Promise<Manifest> {
  return request("/content/manifest");
}

/** 按 uid 取单篇 markdown 正文。 */
export async function getFile(uid: string): Promise<string> {
  const resp = await fetch(`/api/content/file?uid=${encodeURIComponent(uid)}`);
  if (!resp.ok) {
    throw new Error(`读取内容失败（${resp.status}）：${uid}`);
  }
  return resp.text();
}

/** 便捷：主仓库（第一个启用源，优先 articles），编辑器跳转/github 默认定位用。 */
export function primaryRepoOf(sources: SourceWithState[]): {
  owner: string;
  repo: string;
  ref: string;
} | null {
  const enabled = sources.filter((s) => s.enabled);
  const pick = enabled.find((s) => s.collection === "articles") ?? enabled[0] ?? sources[0] ?? null;
  if (!pick) return null;
  return { owner: pick.owner, repo: pick.repo, ref: pick.state.resolved_ref || pick.ref };
}

/** 便捷：按 source 配置生成编辑器跳转路径（无主仓库时返回 null，调用方隐藏入口）。 */
export function editorHrefFor(sources: SourceWithState[], path: string): string | null {
  const repo = primaryRepoOf(sources);
  if (!repo) return null;
  const qs = `?file=${encodeURIComponent(path)}${repo.ref ? `&ref=${encodeURIComponent(repo.ref)}` : ""}`;
  return `/app/github-editor/repo/${repo.owner}/${repo.repo}${qs}`;
}
