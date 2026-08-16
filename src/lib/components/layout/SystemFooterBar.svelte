<!--
	SystemFooterBar：系统底部状态栏（跨路由常驻全宽，与顶部 SystemStatusBar 对偶）。

	正交意图：
	1. 原始需求（2026-08-14）：页面底部全宽状态栏，视觉语言对齐顶部 SystemStatusBar
	   （glass-surface 毛玻璃 + text-xs + border-t），高度减半（h-4.5）。
	   条目右对齐，两两之间用分割线隔开。
	2. 外链可配置（2026-08-16）：消费 siteStore（后端 /api/site 的 [site] 配置，
	   全站生效）——GitHub 源码入口 / ICP 备案号等由部署者在设置「状态栏」中管理；
	   后端不可达时回退 SITE 常量默认。渲染层统一保证新窗口 + noopener noreferrer
	   （备案号合规：链接 beian.miit.gov.cn）。

	未来演进（设计备忘，均未实现）：
	3. 抽屉化：底栏将升级为抽屉（Drawer），顶部中央带一个小手柄（handle），
	   点击后像 Sheet 一样向上展开；收起态保持现有条目可见。
	4. 展开态 · 方案一「数据中转站」：将全站各种功能的数据统一建模为一种
	   类剪贴板的数据结构（可承载任意类型数据），展开后的抽屉里以卡片化形式
	   存储这些数据，成为跨应用的临时数据中转枢纽。
	5. 展开态 · 方案二「AI 入口」：点击可唤起个人 AI 助理对话，借助 WebMCP
	   协议读取/操作 GaubeeOS 上的内容（应用、VFS、通知等），作为系统级 AI 面板。
-->
<script lang="ts">
  import { siteStore } from '$lib/site/site-store.svelte'

  const links = $derived(siteStore.footerLinks)
</script>

<footer class="system-footerbar glass-surface border-border/80 z-[var(--z-shell-base)] shrink-0 border-t">
  <!-- 内容行：高度为顶栏一半（h-4.5 = 18px），条目右对齐、分割线隔开；
       未来手柄（handle）与展开内容也挂在这一层 -->
  <div class="text-muted-foreground flex h-4.5 items-center justify-end gap-2 px-2 text-xs leading-none">
    {#each links as link, i (link.id)}
      {#if i > 0}
        <span class="bg-border h-3 w-px shrink-0" aria-hidden="true"></span>
      {/if}
      <a
        href={link.url}
        target="_blank"
        rel="noopener noreferrer"
        class="hover:text-foreground rounded px-1 transition-colors hover:bg-accent"
      >
        {link.label}
      </a>
    {/each}
  </div>
  <!-- iOS 安全区让位（viewport-fit=cover 下避开 home indicator，非全面屏为 0 高度） -->
  <div class="h-[env(safe-area-inset-bottom)]" aria-hidden="true"></div>
</footer>
