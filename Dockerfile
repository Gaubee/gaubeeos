# ---------------------------------------------------------------------------
# GaubeeOS 镜像：SPA 前端 + Rust 订阅引擎服务端（单容器）。
#
# 三阶段：node 前端构建 → Rust 服务端编译（musl）→ scratch 运行时。
# 有状态：/data 卷（订阅配置 config.toml + 缓存 + 清单）。
# 环境变量：
#   SERVER_ROOT（默认 /srv）PORT（默认 8080）DATA_DIR（默认 /data）
#   GITHUB_TOKEN（可选，提升订阅同步的 API 限额）
#   VITE_AUTH_BASE（构建期，可选：OAuth Worker 域名；未配置 = 纯只读部署）
# ---------------------------------------------------------------------------

# ---- 阶段 1：构建 SPA ----
FROM node:22-alpine AS site
WORKDIR /app

# pnpm@10.22.0：与本地开发环境对齐（更新 10.x 与 workspace 白名单 flag 互斥）
RUN npm install -g pnpm@10.22.0

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc ./
RUN pnpm install --frozen-lockfile

# VITE_AUTH_BASE 可选：配置了才启用 OAuth 登录入口（isAuthConfigured）
ARG VITE_AUTH_BASE=""

COPY . .
RUN if [ -n "$VITE_AUTH_BASE" ]; then export VITE_AUTH_BASE; fi && pnpm build

# ---- 阶段 2：编译 Rust 服务端（alpine musl 静态链接）----
FROM rust:1-alpine AS server
WORKDIR /build
RUN apk add --no-cache musl-dev
# 依赖层单独缓存：仅 Cargo.toml/lock 变更才触发依赖重编
COPY static-server/Cargo.toml static-server/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
  && cargo build --release \
  && rm -rf src target/release/deps/gaubeeos_server*
COPY static-server/src ./src
RUN touch src/main.rs && cargo build --release

# ---- 阶段 3：scratch 运行时（二进制 + SPA 产物 + /data 卷挂载点）----
FROM scratch
COPY --from=server /build/target/release/gaubeeos-server /server
COPY --from=site /app/build /srv

# 非 root 运行（scratch 无 /etc/passwd，数字 UID/GID；8080 非 root 可绑）
# /data 目录由卷挂载创建（scratch 无法 mkdir，VOLUME 声明在 compose 侧）
USER 65532:65532
ENV SERVER_ROOT=/srv PORT=8080 DATA_DIR=/data
EXPOSE 8080
VOLUME ["/data"]
ENTRYPOINT ["/server"]
