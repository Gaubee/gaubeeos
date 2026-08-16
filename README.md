# GaubeeOS

浏览器中的「桌面操作系统」式个人站点内核。订阅 GitHub 仓库作为内容源，任何人都能部署自己的 OS 站点。

```
┌─ gaubeeos-server（Rust, axum + tokio，单容器）──────────────┐
│ ① 静态托管（SPA，四级查找 + 缓存矩阵 + gzip）                 │
│ ② 订阅引擎（有状态）：GitHub 源 → 定时同步 → /data 缓存       │
│ ③ REST API：/api/sources（订阅 CRUD/测试/同步）+ /api/content │
└──────────────────────────────────────────────────────────────┘
前端（SvelteKit SPA）：桌面 OS 交互（应用系统/终端/编辑器/Git 客户端/
Markdown 阅读/全文搜索），内容全部来自订阅引擎
```

## 快速开始（docker compose）

```bash
# 1. 保存 docker-compose.yml 到任意目录（如 /opt/gaubeeos/）
# 2.（可选）.env 配置 GITHUB_TOKEN=ghp_xxx（匿名 60 次/小时，配 token 5000 次/小时）
# 3. 启动
docker compose pull && docker compose up -d
# 4. 打开 http://<host>:8090 → 设置 → 内容源 → 添加 GitHub 仓库订阅
```

订阅示例：订阅本内核的源仓库（自演示）：

| 字段         | 值                                                 |
| ------------ | -------------------------------------------------- |
| owner / repo | `Gaubee` / `gaubeeos` 或任意含 markdown 的公开仓库 |
| 内容类型     | 文章 或 说说                                       |
| include      | `src/content/articles/**/*.md`（路径匹配 glob）    |

include 匹配的文件路径映射为本地 URL：`/article/{内容类型}/{slug 前缀}{文件名去扩展名}`。可订阅多个源（文章/说说各多个），slug 前缀用于多源防冲突。

数据落盘 `./data`（卷）：`config.toml`（订阅配置，可手编）+ `cache/`（markdown 缓存与聚合清单）。升级/重建容器不丢数据；更新即 `docker compose pull && up -d`。

## 内容源字段

| 字段         | 说明                                 | 默认          |
| ------------ | ------------------------------------ | ------------- |
| owner / repo | GitHub 仓库坐标                      | 必填          |
| ref          | 分支 / tag / sha                     | 空 = 默认分支 |
| 内容类型     | `articles`（文章）/ `events`（说说） | 必选          |
| include      | 文件匹配 glob（仓库相对路径）        | 必填          |
| 同步频率     | `15m / 30m / 1h / 6h / 12h / 24h`    | `1h`          |
| slug 前缀    | URL 防冲突前缀                       | 空            |
| 启停         | 停用 = 停止同步（内容保留）          | 启用          |

同步算法：分支 head sha 未变则跳过（每轮 REST ≤3 次）；变化时 Trees API 增量对比 blob sha，仅下载变化文件（正文走 raw CDN，不占 REST 限额）。

## 本地开发

```bash
# 服务端（订阅引擎，:8090）
cd static-server
DATA_DIR=./.data PORT=8090 SERVER_ROOT=../build GITHUB_TOKEN=$(gh auth token) cargo run

# 前端（:5174，/api 与 /auth 经 vite proxy 转发）
pnpm install && pnpm exec vite dev --port 5174

# 测试
cargo test                      # 服务端 12 单测
pnpm test:unit --run            # 前端 445 单测
pnpm check                      # 类型检查
```

## 权限模型（manager，2026-08-17）

```
GitHub OAuth（唯一可信源，worker 零改动）
  → 前端 POST /api/session {gh_token}      ← 总钥匙只出现这一次
  → 后端 GET /user 验证 → 签发 HttpOnly cookie 会话（刷新不掉线）
  → role = login ∈ MANAGER_GITHUB_ACCOUNTS ? manager : user/anonymous
```

- **能力 = store 可访问性**：managerStoreService（`/api/store/{ns}`，namespace=appId，
  5MB 配额）manager 可写、匿名可读（站点默认需要）。所有写 API（site/sources/store）
  统一 manager 门禁（401 未登录 / 403 非管理员）。
- **应用级**：manifest `managerOnly: true` → 非管理员隐藏（桌面/启动器，fail-closed）
  - 深链引导页。主题应用为试点（站点级外观）。
- **分层配置**：站点层（managerStore）为默认，localStorage 为个人覆盖（local > site）；
  未来演进 userStoreService（githubStoreProvider）时把个人层搬上服务端即可。

### 环境变量

| env                       | 说明                                                           |
| ------------------------- | -------------------------------------------------------------- |
| `MANAGER_GITHUB_ACCOUNTS` | 管理员 GitHub login（逗号分隔，大小写不敏感）                  |
| `MANAGER_STORE_MODE`      | `local`（默认）/ `git`                                         |
| `MANAGER_STORE_REPO`      | git 模式：`owner/repo`（写入凭据复用 GITHUB_TOKEN）            |
| `MANAGER_STORE_PATH`      | git 模式：仓库内文件夹（默认 `gaubeeos-store/`，每 ns 一文件） |
| `DEV_SESSION_TOKEN`       | 本地开发：任意值注册为 manager 会话（生产勿配）                |
| `COOKIE_SECURE`           | https 部署时配置（cookie 加 Secure）                           |

git 模式：内存先行（PUT 即生效）→ 本地镜像 → per-ns 3s 防抖 commit（sha 乐观锁 +
409 重试）；崩溃恢复以本地镜像为缓冲真相（dirty 标记胜出重排 flush）。

## OAuth 登录（可选）

编辑器写入/Git 提交需要 GitHub 登录，依赖 `worker/`（Cloudflare Worker，OAuth 中继 + 图片上传）。部署者自建：复制 `worker/`，配置 wrangler 的 `APP_ORIGIN`/`WORKER_ORIGIN` 与 GitHub OAuth App，构建前端时注入 `VITE_AUTH_BASE`。未配置时为纯只读部署（登录入口自动隐藏，浏览与订阅不受影响）。

## 与 [gaubee.com](https://github.com/Gaubee/gaubee.com) 的关系

gaubeeos 是从 gaubee.com 剥离的通用内核（2026-08）。gaubee.com 是内核 + 博主内容的单体形态，继续独立服役；本仓库面向任何人部署，内容全部运行时订阅，不内置任何站点的文章。

## 技术栈

SvelteKit（SPA）· Svelte 5 runes · Tailwind v4 · shadcn-svelte · axum + tower-http + reqwest（Rust 服务端）· MiniSearch（运行时全文搜索）· ZenFS/IndexedDB（可写 VFS）· xterm.js · CodeMirror 6

## License

MIT
