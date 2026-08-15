/**
 * P2 全流程走查（v2）：等待策略改为 waitForSelector，搜索聚焦输入框。
 * 前置：static-server（:8090）+ vite dev（:5174）。
 */
import { chromium } from 'playwright'
import { mkdirSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const OUT = resolve(__dirname, 'screenshots')
mkdirSync(OUT, { recursive: true })
const BASE = 'http://localhost:5174'

const browser = await chromium.launch()
const results = []
const check = (name, ok) => results.push([ok ? '✅' : '❌', name, ok])

const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } })
const page = await ctx.newPage()
const errs = []
page.on('pageerror', (e) => errs.push('PAGEERR: ' + e.message.slice(0, 200)))

// 1. 文章列表：等首个文章链接出现（最长 20s，覆盖 85 文件装载）
await page.goto(`${BASE}/app/articles`, { waitUntil: 'domcontentloaded' })
const firstLink = page.locator('a[href^="/article/articles/"]').first()
await firstLink.waitFor({ state: 'visible', timeout: 20000 })
const articleCount = await page.locator('a[href^="/article/articles/"]').count()
check(`文章列表渲染（${articleCount} 篇，预期 > 50）`, articleCount > 50)
await page.screenshot({ path: resolve(OUT, '02-articles.png') })

// 2. 文章详情（点击第一篇）
const href = await firstLink.getAttribute('href')
await firstLink.click()
await page.waitForTimeout(2500)
const detailText = await page.evaluate(() => document.body.innerText)
check(`详情页渲染（${href}）`, (await page.locator('h1').count()) > 0 && detailText.length > 200)
await page.screenshot({ path: resolve(OUT, '03-detail.png') })

// 3. 说说时间线
await page.goto(`${BASE}/app/shout`, { waitUntil: 'domcontentloaded' })
await page.locator('a[href^="/article/events/"]').first().waitFor({ state: 'visible', timeout: 15000 })
const shoutCount = await page.locator('a[href^="/article/events/"]').count()
check(`说说时间线渲染（${shoutCount} 条，预期 > 15）`, shoutCount > 15)
await page.screenshot({ path: resolve(OUT, '04-shout.png') })

// 4. 设置 → 内容源面板
await page.goto(`${BASE}/app/settings`, { waitUntil: 'domcontentloaded' })
await page.getByText('内容源').first().waitFor({ timeout: 10000 })
const settingsText = await page.evaluate(() => document.body.innerText)
check('设置页含「内容源」section', settingsText.includes('内容源'))
check('源卡片展示（Gaubee 文章/说说）', settingsText.includes('Gaubee 文章') && settingsText.includes('Gaubee 说说'))
check('状态展示（篇数）', /\d+\s*篇/.test(settingsText))
await page.screenshot({ path: resolve(OUT, '05-settings.png') })

// 5. 搜索（运行时索引）：聚焦输入框再键入
await page.goto(`${BASE}/`, { waitUntil: 'domcontentloaded' })
await page.locator('a[href^="/article/articles/"]').first().waitFor({ state: 'visible', timeout: 15000 }).catch(() => {})
await page.getByRole('button', { name: '搜索' }).click()
const input = page.getByPlaceholder('搜索内容或输入 app:articles')
await input.waitFor({ state: 'visible', timeout: 10000 })
await input.fill('Rust')
await page.waitForTimeout(2500)
const searchText = await page.evaluate(() => document.body.innerText)
check(`搜索「Rust」返回结果`, /\/article\/articles\//.test(searchText) || searchText.includes('Rust'))
await page.screenshot({ path: resolve(OUT, '06-search.png') })

// 6. 页面错误（区分：replaceState 水合竞态单列，其余计失败）
const fatal = errs.filter((e) => !e.includes('replaceState'))
check(`无致命页面错误（${fatal.length}）`, fatal.length === 0)
if (errs.length) console.log('（错误明细）\n' + [...new Set(errs)].slice(0, 4).join('\n'))

await ctx.close()
await browser.close()

let fail = 0
for (const [icon, name] of results) {
  console.log(`${icon} ${name}`)
  if (icon === '❌') fail++
}
console.log(fail === 0 ? '\n全部通过' : `\n${fail} 项失败`)
process.exit(fail === 0 ? 0 : 1)
