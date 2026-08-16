/**
 * ThemeService：主题色相能力（GaubeeOS 应用服务总线的一部分）。
 *
 * 分层存储（2026-08-17，managerStore 试点）：
 * - 站点层：GET/PUT /api/store/theme（管理员设置的全站默认，匿名可读）
 * - 个人层：localStorage（原行为保留；当前作为浏览器覆盖，未来演进为 userStore）
 * 解析顺序：站点默认 → 本地覆盖（local > site）。管理员写入站点层并清本地覆盖；
 * 非管理员写入本地层（主题应用虽 managerOnly，此能力保留给未来 userStore 演进）。
 *
 * 管理运行时双旋钮色相：
 * - --primary-h：品牌强调色（橙红系），驱动 primary / chart 系列 / sidebar-primary。
 * - --base-h：中性表面色（mauve 系），驱动 foreground / muted / border / ring 等中性色。
 *
 * 色彩学规律（参考 shadcn-svelte base color 体系）：
 * - 中性色的 H = base-h，C 极低（0.001-0.034），L 按表面层级固定。
 * - 亮模式 background 永远纯白，不随 base 变；暗模式 background 带 base hue。
 * - primary/destructive 独立于 base（品牌色/危险色各有自己的语义）。
 *
 * 亮度/彩度由 CSS 字面量锁定（可访问性保证），用户只能旋转色相。
 *
 * 正交意图：
 * 1. 原始需求（2026-07-24）：自定义 primary color，保持与默认色一致的亮度。
 * 2. 扩展（2026-07-25）：新增 base color 旋钮，中性表面色可独立调整。
 */
import { browser } from "$app/environment";
import { backendSession } from "$lib/auth/backend-session.svelte";
import type { AppService } from "$lib/os/services";

/** 默认 primary 色相（橙红，与 app.css :root 一致）。 */
export const DEFAULT_PRIMARY_HUE = 16.935;
/** 默认 base 色相（mauve，与 app.css :root 一致）。 */
export const DEFAULT_BASE_HUE = 326;

/** localStorage key。 */
const STORAGE_KEY = "gaubee:os:theme";

/** 持久化结构。 */
interface PersistedTheme {
  hue: number;
  baseHue?: number;
}

/** 主题服务接口。 */
export interface ThemeService extends AppService {
  readonly id: "theme";
  readonly appId: "theme";
  /** 当前 primary 色相（品牌色，0-360）。 */
  readonly hue: number;
  /** 当前 base 色相（中性色，0-360）。 */
  readonly baseHue: number;
  /** 设置 primary 色相，即时注入 --primary-h 并持久化。 */
  setHue(hue: number): void;
  /** 设置 base 色相，即时注入 --base-h 并持久化。 */
  setBaseHue(hue: number): void;
  /** 重置为默认色相（primary + base）。 */
  reset(): void;
  /** 从站点层（managerStore）装载默认值（boot 调用；本地覆盖仍优先生效）。 */
  loadSiteDefaults(): Promise<void>;
  /** 清除本浏览器的个人覆盖层（回到站点默认）。 */
  clearLocalOverride(): void;
  /** 是否存在本地覆盖。 */
  readonly hasLocalOverride: boolean;
}

/** 归一化色相到 [0, 360)。 */
function normalizeHue(hue: number): number {
  return ((hue % 360) + 360) % 360;
}

/** 站点层 ns。 */
const SITE_NS = "theme";

class ThemeServiceImpl implements ThemeService {
  readonly id = "theme" as const;
  readonly appId = "theme" as const;

  hue = $state(DEFAULT_PRIMARY_HUE);
  baseHue = $state(DEFAULT_BASE_HUE);
  hasLocalOverride = $state(false);

  constructor() {
    if (browser) {
      this.restore();
    }
  }

  /** 站点层装载：GET /api/store/theme → 作为默认值（仅当无本地覆盖时生效到 DOM）。 */
  async loadSiteDefaults(): Promise<void> {
    if (!browser) return;
    try {
      const resp = await fetch(`/api/store/${SITE_NS}`);
      if (!resp.ok) return;
      const v = (await resp.json()) as PersistedTheme;
      if (typeof v.hue === "number") {
        this.hue = normalizeHue(v.hue);
        this.applyPrimaryToDom(this.hue);
      }
      if (typeof v.baseHue === "number") {
        this.baseHue = normalizeHue(v.baseHue);
        this.applyBaseToDom(this.baseHue);
      }
      // 本地覆盖重新套用（local > site）
      this.restore();
      this.notifySw();
    } catch {
      // 后端不可达：保持出厂默认
    }
  }

  clearLocalOverride(): void {
    if (!browser) return;
    localStorage.removeItem(STORAGE_KEY);
    this.hasLocalOverride = false;
    void this.loadSiteDefaults();
  }

  setHue(hue: number): void {
    const normalized = normalizeHue(hue);
    this.hue = normalized;
    this.applyPrimaryToDom(normalized);
    this.persist();
    this.notifySw();
  }

  setBaseHue(hue: number): void {
    const normalized = normalizeHue(hue);
    this.baseHue = normalized;
    this.applyBaseToDom(normalized);
    this.persist();
    this.notifySw();
  }

  reset(): void {
    this.hue = DEFAULT_PRIMARY_HUE;
    this.baseHue = DEFAULT_BASE_HUE;
    this.applyPrimaryToDom(DEFAULT_PRIMARY_HUE);
    this.applyBaseToDom(DEFAULT_BASE_HUE);
    this.persist();
    this.notifySw();
  }

  /** 注入 --primary-h 到 documentElement（驱动 app.css primary 系计算式）。 */
  private applyPrimaryToDom(hue: number): void {
    if (!browser) return;
    document.documentElement.style.setProperty("--primary-h", String(hue));
  }

  /** 注入 --base-h 到 documentElement（驱动 app.css 中性色计算式）。 */
  private applyBaseToDom(hue: number): void {
    if (!browser) return;
    document.documentElement.style.setProperty("--base-h", String(hue));
  }

  /**
   * 通知 Service Worker 主题色变更（增强：SW 可在首屏 HTML 注入色相，杜绝刷新闪烁）。
   * SW 不存在时无副作用（dev 模式不注册 SW）。
   */
  private notifySw(): void {
    if (!browser) return;
    try {
      navigator.serviceWorker?.controller?.postMessage({
        type: "THEME_HUE",
        hue: this.hue,
        baseHue: this.baseHue,
      });
    } catch {
      // SW 不可用，忽略（增强特性，失败不影响核心功能）
    }
  }

  /** 写入策略：管理员 → 站点层（成功后清本地覆盖，保持单一真相）；
   * 非管理员 → 本地层（未来 userStore 的演进位）。 */
  private persist(): void {
    if (!browser) return;
    const data: PersistedTheme = { hue: this.hue, baseHue: this.baseHue };
    if (backendSession.isManager) {
      void fetch(`/api/store/${SITE_NS}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data),
      })
        .then((resp) => {
          if (resp.ok) {
            localStorage.removeItem(STORAGE_KEY);
            this.hasLocalOverride = false;
          } else {
            // 站点层写失败（会话过期等）→ 退本地层，不丢用户操作
            this.persistLocal(data);
          }
        })
        .catch(() => this.persistLocal(data));
    } else {
      this.persistLocal(data);
    }
  }

  private persistLocal(data: PersistedTheme): void {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
      this.hasLocalOverride = true;
    } catch {
      // 存储不可用，忽略
    }
  }

  private restore(): void {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) {
        this.hasLocalOverride = false;
        return;
      }
      this.hasLocalOverride = true;
      const parsed = JSON.parse(raw) as unknown;
      if (parsed !== null && typeof parsed === "object") {
        const obj = parsed as PersistedTheme;
        if (typeof obj.hue === "number") {
          this.hue = normalizeHue(obj.hue);
          this.applyPrimaryToDom(this.hue);
        }
        if (typeof obj.baseHue === "number") {
          this.baseHue = normalizeHue(obj.baseHue);
          this.applyBaseToDom(this.baseHue);
        }
        this.notifySw();
      }
    } catch {
      // 损坏数据，忽略
    }
  }
}

/** 主题服务单例。 */
export const themeService: ThemeService = new ThemeServiceImpl();
