/**
 * GaubeeOS Service Worker —— SPA 首屏主题态注入。
 *
 * 内核版（2026-08-16）：SSG /pages 已随内容订阅化移除，SW 职责收敛为：
 * 1. SPA navigation 请求（Accept: text/html）：网络优先 + 主题态注入，
 *    在返回的 HTML <head> 末尾注入 <style>--primary-h/--base-h</style>，
 *    杜绝刷新主题闪烁（增强）。
 * 2. 主题持久化：client postMessage({type:"THEME_HUE"}) → Cache Storage，
 *    fetch 时读取注入。SW 不存在时（dev）走 JS 注入路径，向后兼容。
 *
 * 注册：见 src/lib/sw/register.ts，仅在 production + browser 注册（dev 不破坏 HMR）。
 */

const CACHE_VERSION = "gaubeeos-sw-v1";
const CACHE_KEY = `gaubeeos-cache-${CACHE_VERSION}`;
/**
 * 主题色相持久化的特殊 cache key。
 * 必须是合法 URL 路径（Cache.put 会解析为 Request），用 / 开头避免协议解析错误。
 * 用特殊前缀 __sw__/ 避免与真实路由冲突。
 */
const THEME_CACHE_KEY = "/__sw__/theme-hue";

// install：无预缓存（SPA 入口不可离线），立即激活
self.addEventListener("install", () => {
  self.skipWaiting();
});

// activate：清理旧版本缓存，立即接管
self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const keys = await caches.keys();
      await Promise.all(keys.filter((k) => k !== CACHE_KEY).map((k) => caches.delete(k)));
      await self.clients.claim();
    })(),
  );
});

// fetch：只接管 SPA navigation（主题注入）
self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);

  if (url.origin !== self.location.origin) return;

  if (
    request.method === "GET" &&
    (request.headers.get("accept") || "").includes("text/html")
  ) {
    event.respondWith(spaNavigationWithTheme(request));
  }
});

/**
 * SPA navigation：网络优先，拿到 HTML 后注入当前主题色相（双旋钮）。
 * 注入 <style>:root{--primary-h:X;--base-h:Y}</style> 到 </head> 前，首屏即带主题态。
 */
async function spaNavigationWithTheme(request) {
  try {
    const response = await fetch(request);
    if (!response.ok) return response;

    const theme = await readThemeHue();
    if (theme === null) return response; // 无主题态，原样返回

    const html = await response.text();
    const rules = [];
    if (typeof theme.hue === "number") rules.push(`--primary-h:${theme.hue}`);
    if (typeof theme.baseHue === "number") rules.push(`--base-h:${theme.baseHue}`);
    if (rules.length === 0) return response;
    const injected = html.replace(
      "</head>",
      `<style id="sw-theme">:root{${rules.join(";")}}</style></head>`,
    );

    return new Response(injected, {
      status: response.status,
      statusText: response.statusText,
      headers: response.headers,
    });
  } catch {
    return new Response("网络不可用", {
      status: 503,
      headers: { "Content-Type": "text/plain; charset=utf-8" },
    });
  }
}

// ---- 主题色相持久化（Cache Storage）----

/** 读取持久化的主题色相 {hue, baseHue}。SW 重启后从 Cache 恢复。 */
async function readThemeHue() {
  const cache = await caches.open(CACHE_KEY);
  const res = await cache.match(THEME_CACHE_KEY);
  if (!res) return null;
  try {
    return await res.json();
  } catch {
    return null;
  }
}

/** 持久化主题色相到 Cache Storage。存 {hue, baseHue} 对象。 */
async function writeThemeHue(theme) {
  const cache = await caches.open(CACHE_KEY);
  const res = new Response(JSON.stringify(theme), {
    headers: { "Content-Type": "application/json" },
  });
  await cache.put(THEME_CACHE_KEY, res);
}

// message：接收 client 的主题更新通知 + SKIP_WAITING
self.addEventListener("message", async (event) => {
  const data = event.data;
  if (data === "SKIP_WAITING") {
    self.skipWaiting();
    return;
  }
  if (data?.type === "THEME_HUE") {
    const theme = {
      hue: typeof data.hue === "number" ? data.hue : undefined,
      baseHue: typeof data.baseHue === "number" ? data.baseHue : undefined,
    };
    try {
      await writeThemeHue(theme);
      if (event.source) {
        event.source.postMessage({ type: "THEME_HUE_ACK", ...theme });
      }
    } catch (err) {
      console.error("[SW] themeHue 持久化失败:", err);
    }
  }
});
