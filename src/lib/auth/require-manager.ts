import { notifyError, notifyInfo } from "$lib/apps/builtin/notifications/service.svelte";
import { navController } from "$lib/nav/nav-controller-instance";

/**
 * 写操作守卫（manager 权限体系的 UX 层）。
 *
 * 真正的安全边界在后端 API（require_manager → 401/403）；本 helper 在操作前
 * 提前给出人话反馈：未登录 → 引导登录；已登录非管理员 → 明确提示。
 */
import { backendSession } from "./backend-session.svelte";

/** 返回 true = 放行（当前用户是管理员）。 */
export function requireManagerOrNotify(): boolean {
  if (backendSession.isManager) return true;
  if (!backendSession.loaded || !backendSession.authenticated) {
    notifyInfo("此操作需要管理员登录");
    navController.navigateMain("/app/account");
  } else {
    notifyError(`当前用户 ${backendSession.login} 非管理员，无相关权限`);
  }
  return false;
}
