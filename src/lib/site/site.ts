/**
 * 站点级展示常量（单一事实源，SPA 底部状态栏消费）。
 *
 * GaubeeOS 内核剥离（2026-08-16）：ICP 备案是部署者属地合规事项，不属于内核。
 * beian 置 null 时底部状态栏不渲染备案链接；部署者如需展示，
 * fork 后在此配置（规范：链接 https://beian.miit.gov.cn/，新窗口 + noopener）。
 */

/** 站点展示信息 */
export const SITE = {
  /** GitHub 源码仓库 */
  githubUrl: "https://github.com/Gaubee/gaubeeos",
  /** ICP 备案展示（null = 不渲染；部署者按属地法规自行配置） */
  beian: null as { label: string; url: string } | null,
} as const;
