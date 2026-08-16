/**
 * 后端会话状态（manager 权限体系的消费端，2026-08-17）。
 *
 * 模型见 static-server/src/session.rs：GitHub token 一次性交换为 HttpOnly cookie，
 * 此后身份由 cookie 承载（刷新不掉线）。role 每次由后端按 env 重算。
 *
 * 职责：
 * - boot `load()`：GET /api/session 恢复身份（cookie 存在则免登录）
 * - 登录回跳后 `syncFromAuth()`：拿 authStore 内存中的 gh token 调一次交换
 * - `logout()`：DELETE 清会话
 * - 派生：role / isManager（应用可见性与写操作守卫消费）
 */
import { browser } from "$app/environment";

import { authStore } from "./session.svelte";

export type BackendRole = "manager" | "user" | "anonymous";

interface SessionPayload {
  authenticated: boolean;
  login: string | null;
  role: BackendRole;
}

class BackendSession {
  loaded = $state(false);
  authenticated = $state(false);
  login = $state<string | null>(null);
  role = $state<BackendRole>("anonymous");

  /** fail-closed：未确认身份前一律视为无权限。 */
  get isManager(): boolean {
    return this.loaded && this.role === "manager";
  }

  private apply(p: SessionPayload): void {
    this.authenticated = p.authenticated;
    this.login = p.login;
    this.role = p.role;
    this.loaded = true;
  }

  /** 恢复会话（boot；匿名也 200）。 */
  async load(): Promise<void> {
    if (!browser) return;
    try {
      const resp = await fetch("/api/session");
      this.apply((await resp.json()) as SessionPayload);
    } catch {
      // 后端不可达：保持 anonymous（fail-closed）
      this.authenticated = false;
      this.login = null;
      this.role = "anonymous";
      this.loaded = true;
    }
  }

  /**
   * gh token → 会话交换（登录回跳后调用一次；总钥匙仅此一刻上网络）。
   * 交换成功后 cookie 由浏览器持有，gh token 继续留 authStore 内存供 GitHub 直连。
   */
  async syncFromAuth(): Promise<void> {
    if (!browser) return;
    const ghToken = authStore.getToken();
    if (!ghToken) {
      // 无 gh token（刷新后的 cookie 会话）→ 直接恢复
      await this.load();
      return;
    }
    try {
      const resp = await fetch("/api/session", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ token: ghToken }),
      });
      if (resp.ok) {
        this.apply((await resp.json()) as SessionPayload);
        return;
      }
    } catch {
      // 交换失败（后端不可达等）→ 尝试恢复既有 cookie 会话
    }
    await this.load();
  }

  /** 登出（与 authStore.logout 联动，由 accountService 调用）。 */
  async logout(): Promise<void> {
    if (!browser) return;
    try {
      await fetch("/api/session", { method: "DELETE" });
    } catch {
      // 忽略：cookie 过期由服务端 Max-Age 兜底
    }
    this.authenticated = false;
    this.login = null;
    this.role = "anonymous";
  }
}

/** 全局单例。 */
export const backendSession = new BackendSession();
