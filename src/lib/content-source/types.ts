/**
 * 内容源订阅的前端类型（与 static-server 的 /api/* DTO 对齐）。
 *
 * 结构镜像：
 * - Rust config.rs：SourceConfig（snake_case 与后端 JSON 一致）
 * - Rust sync.rs：SourceState / SyncOutcome
 * - Rust manifest.rs：ManifestEntry / Manifest / SourceRef
 */

/** 内容集合（与后端 Collection 枚举的字符串形式对齐）。 */
export type Collection = "articles" | "events";

/** 订阅配置（后端 config.toml 的一行 source）。 */
export interface SourceConfig {
  id: string;
  name?: string;
  owner: string;
  repo: string;
  /** Git ref（空 = 默认分支）。 */
  ref: string;
  collection: Collection;
  /** 文件匹配 glob（仓库相对路径）。 */
  include: string;
  exclude?: string;
  slug_prefix?: string;
  /** humantime 语法（15m/1h/6h/24h…）。 */
  interval: string;
  enabled: boolean;
}

/** 每源运行态（后端 state/{id}.json）。 */
export interface SourceState {
  last_sha?: string;
  resolved_ref?: string;
  last_sync_at?: string;
  last_error?: string;
  file_shas?: Record<string, string>;
  entries?: ManifestEntry[];
}

/** 列表接口返回的完整源（配置 + 运行态 + 展示名）。 */
export interface SourceWithState {
  display_name: string;
  state: SourceState;
  [key: string]: unknown;
  id: string;
  name?: string;
  owner: string;
  repo: string;
  ref: string;
  collection: Collection;
  include: string;
  exclude?: string;
  slug_prefix?: string;
  interval: string;
  enabled: boolean;
}

/** 单轮同步结果。 */
export interface SyncOutcome {
  skipped: boolean;
  head_sha: string;
  downloaded: number;
  removed: number;
  total_files: number;
  error?: string | null;
}

/** 测试连接结果（不下载）。 */
export interface TestResult {
  head_sha: string;
  resolved_ref: string;
  matched: number;
  sample: string[];
}

/** 源归属（清单条目内嵌）。 */
export interface SourceRef {
  id: string;
  owner: string;
  repo: string;
  ref: string;
}

/** 聚合清单条目（后端 manifest.json）。 */
export interface ManifestEntry {
  uid: string;
  source: SourceRef;
  collection: string;
  path: string;
  filename: string;
  slug: string;
  slug_prefix: string;
  title?: string;
  date?: string;
  updated?: string;
  tags: string[];
  bytes: number;
  synced_at: string;
}

/** 聚合清单。 */
export interface Manifest {
  version: number;
  generated_at: string;
  entries: ManifestEntry[];
}

/** 创建/更新订阅的请求体。 */
export interface SourceInput {
  name?: string;
  owner: string;
  repo: string;
  ref?: string;
  collection: Collection;
  include: string;
  exclude?: string;
  slug_prefix?: string;
  interval?: string;
  enabled?: boolean;
}
