/**
 * 只读内容数据（内核版桩）。
 *
 * gaubee.com 单体模式：构建脚本 build-readonly-vfs.ts 把 src/content 全量
 * markdown 打进 bundle（此处导出非空 Map）。
 *
 * GaubeeOS 内核模式（2026-08-16）：内容来自后端订阅引擎（static-server），
 * 前端经 /api/content/* 拉取（见 content-pipeline 的动态 source），
 * 此处恒为空——保留导出以维持 readonly.ts 的接口兼容（P2 移除）。
 */
export const readonlyFiles: Record<string, string> = {};
