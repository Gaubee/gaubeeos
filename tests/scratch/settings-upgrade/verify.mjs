import { mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * 系统化管理升级走查：macOS 式设置 + 应用设置拆分 + 状态栏外链配置。
 * 前置：原生 gaubeeos-server（:28090，含订阅数据）。
 */
import { chromium } from "playwright";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(__dirname, "screenshots");
mkdirSync(OUT, { recursive: true });
const BASE = "http://127.0.0.1:28090";

const browser = await chromium.launch();
const results = [];
const check = (name, ok) => results.push([ok ? "✅" : "❌", name, ok]);

const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
const page = await ctx.newPage();
const errs = [];
page.on("pageerror", (e) => errs.push("PAGEERR: " + e.message.slice(0, 150)));

// 1. /app/settings 默认面板（appearance）+ 双栏布局
await page.goto(`${BASE}/app/settings`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(2500);
let text = await page.evaluate(() => document.body.innerText);
check("设置页双栏（系统/应用分组标题）", text.includes("系统") && text.includes("应用"));
check(
  "默认面板 = 外观（首个 system section）",
  text.includes("切换到亮色") || text.includes("切换到暗色"),
);
check("应用组含 文章源/说说源", text.includes("文章源") && text.includes("说说源"));
check("系统组含 状态栏", text.includes("状态栏"));
await page.screenshot({ path: resolve(OUT, "01-settings.png") });

// 2. 文章源子页（深链）：仅文章源卡片
await page.goto(`${BASE}/app/settings/articles.sources`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(2000);
text = await page.evaluate(() => document.body.innerText);
check("文章源面板标题", text.includes("文章源"));
check(
  "仅列 articles 源（Gaubee 文章）",
  text.includes("Gaubee 文章") && !text.includes("Gaubee 说说"),
);
await page.screenshot({ path: resolve(OUT, "2-articles-sources.png") });

// 3. 说说源子页：仅 events
await page.goto(`${BASE}/app/settings/shout.sources`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(1500);
text = await page.evaluate(() => document.body.innerText);
check(
  "说说源面板（Gaubee 说说，无文章源）",
  text.includes("Gaubee 说说") && !text.includes("Gaubee 文章"),
);

// 4. 状态栏面板：添加备案号链接并保存
await page.goto(`${BASE}/app/settings/statusbar`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(1500);
await page.getByRole("button", { name: "添加外链" }).click();
const label = page.locator('input[id^="fl-label-"]').last();
const url = page.locator('input[id^="fl-url-"]').last();
await label.fill("闽ICP备17026139号-1");
await url.fill("https://beian.miit.gov.cn/");
await page.getByRole("button", { name: "添加外链" }).click();
await page.locator('input[id^="fl-label-"]').last().fill("GitHub");
await page.locator('input[id^="fl-url-"]').last().fill("https://github.com/Gaubee/gaubeeos");
await page.screenshot({ path: resolve(OUT, "3-statusbar-editing.png") });
await page.getByRole("button", { name: "保存", exact: false }).first().click();
await page.waitForTimeout(1500);
await page.screenshot({ path: resolve(OUT, "4-statusbar-saved.png") });

// 5. 底部状态栏即时生效（刷新首页验证持久化 + 渲染）
await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(2500);
const footer = page.locator(".system-footerbar");
const footerText = await footer.innerText();
check(
  "底栏出现备案号 + GitHub 链接",
  footerText.includes("闽ICP备17026139号-1") && footerText.includes("GitHub"),
);
const beianHref = await footer
  .getByRole("link", { name: "闽ICP备17026139号-1" })
  .getAttribute("href");
check(`备案号链接指向工信部（${beianHref}）`, beianHref === "https://beian.miit.gov.cn/");
await page.screenshot({ path: resolve(OUT, "5-footer-links.png") });

// 6. 未知 section 兜底
await page.goto(`${BASE}/app/settings/nonexistent`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(1500);
text = await page.evaluate(() => document.body.innerText);
check("未知面板兜底提示", text.includes("未知的设置面板"));
await page.screenshot({ path: resolve(OUT, "6-unknown-section.png") });

// 7. 内容应用不受影响
await page.goto(`${BASE}/app/articles`, { waitUntil: "domcontentloaded" });
await page
  .locator('a[href^="/article/articles/"]')
  .first()
  .waitFor({ state: "visible", timeout: 15000 });
check("文章列表正常", (await page.locator('a[href^="/article/articles/"]').count()) > 50);

check("全程无致命页面错误", errs.filter((e) => !e.includes("replaceState")).length === 0);
if (errs.length) console.log([...new Set(errs)].slice(0, 3).join("\n"));

await ctx.close();
await browser.close();

let fail = 0;
for (const [icon, name] of results) {
  console.log(`${icon} ${name}`);
  if (icon === "❌") fail++;
}
console.log(fail === 0 ? "\n全部通过" : `\n${fail} 项失败`);
process.exit(fail === 0 ? 0 : 1);
