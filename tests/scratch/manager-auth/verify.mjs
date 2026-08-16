import { mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Manager 权限体系前端走查：匿名隐藏 / 深链引导 / dev 会话可见 + 写入 / 站点层主题。
 * 前置：原生 server（:28090，MANAGER_GITHUB_ACCOUNTS=gaubee DEV_SESSION_TOKEN=dev-test-token-123），
 * 且已通过 curl PUT /api/store/theme {hue:210, baseHue:300}。
 */
import { chromium } from "playwright";

const __dirname = dirname(fileURLToPath(import.meta.url));
mkdirSync(resolve(__dirname, "screenshots"), { recursive: true });
const OUT = resolve(__dirname, "screenshots");
const BASE = "http://127.0.0.1:28090";
const COOKIE = {
  name: "gaubeeos_session",
  value: "dev-test-token-123",
  url: BASE + "/api/session",
};

const browser = await chromium.launch();
const results = [];
const check = (name, ok) => results.push([ok ? "✅" : "❌", name, ok]);

// ===== 匿名上下文 =====
const anon = await browser.newContext({ viewport: { width: 1280, height: 800 } });
const ap = await anon.newPage();
const aerrs = [];
ap.on("pageerror", (e) => aerrs.push(e.message.slice(0, 120)));

await ap.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await ap.waitForTimeout(3000);
const anonText = await ap.evaluate(() => document.body.innerText);
check(
  "匿名桌面无「主题」应用（managerOnly 隐藏）",
  !anonText.includes("主题") || anonText.includes("主题色"),
);
await ap.screenshot({ path: resolve(OUT, "1-anon-desktop.png") });

// 深链 /app/theme → 守卫引导
await ap.goto(`${BASE}/app/theme`, { waitUntil: "domcontentloaded" });
await ap.waitForTimeout(2500);
const guardText = await ap.evaluate(() => document.body.innerText);
check("深链 /app/theme → 管理员引导页", guardText.includes("仅管理员可用"));
await ap.screenshot({ path: resolve(OUT, "2-anon-guard.png") });

// 站点层主题生效（匿名：hue=210 注入 CSS 变量）
await ap.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await ap.waitForTimeout(2500);
const hue = await ap.evaluate(() =>
  getComputedStyle(document.documentElement).getPropertyValue("--primary-h").trim(),
);
check(`站点层主题 hue=210 生效（实际 ${hue}）`, hue === "210");
check("匿名无致命页面错误", aerrs.filter((e) => !e.includes("replaceState")).length === 0);
await anon.close();

// ===== manager 上下文（dev cookie）=====
const mctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
await mctx.addCookies([COOKIE]);
const mp = await mctx.newPage();
const merrs = [];
mp.on("pageerror", (e) => merrs.push(e.message.slice(0, 120)));

await mp.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await mp.waitForTimeout(3000);
const mText = await mp.evaluate(() => document.body.innerText);
check("manager 桌面可见「主题」应用", mText.includes("主题"));
await mp.screenshot({ path: resolve(OUT, "3-manager-desktop.png") });

// 打开主题应用 → 正常渲染（非守卫页）
await mp.goto(`${BASE}/app/theme`, { waitUntil: "domcontentloaded" });
await mp.waitForTimeout(2500);
const themeText = await mp.evaluate(() => document.body.innerText);
check("manager 打开主题应用正常（非守卫页）", !themeText.includes("仅管理员可用"));
await mp.screenshot({ path: resolve(OUT, "4-manager-theme.png") });

// 会话身份
const session = await mp.evaluate(async () => {
  const r = await fetch("/api/session");
  return r.json();
});
check(
  `manager 会话 role（${session.role}/${session.login}）`,
  session.role === "manager" && session.login === "gaubee",
);

check("manager 无致命页面错误", merrs.filter((e) => !e.includes("replaceState")).length === 0);
await mctx.close();
await browser.close();

let fail = 0;
for (const [icon, name] of results) {
  console.log(`${icon} ${name}`);
  if (icon === "❌") fail++;
}
console.log(fail === 0 ? "\n全部通过" : `\n${fail} 项失败`);
process.exit(fail === 0 ? 0 : 1);
