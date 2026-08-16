<!--
	SystemMenuButton：LOGO 系统菜单（苹果菜单）触发器。

	2026-08-17 从 SystemStatusBar 迁到 DesktopSidebar 底部（原顶部左角）。
	菜单内容 = 各应用注册的 system 菜单拍平一列 + 可见性过滤 + 分隔符归一
	（登录区动态化见 account manifest 的 visible 字段）。
-->
<script lang="ts">
  import { navController } from '$lib/nav/nav-controller-instance'
  import { appManager } from '$lib/apps/AppManager.svelte'
  import { appMenuRegistry } from '$lib/apps/menu/registry'
  import type { AppMenuItem } from '$lib/apps/menu/types'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'

  const logoUrl = '/favicon-32.png'

  const systemMenus = $derived(appMenuRegistry.forPlacement('system'))

  /** 菜单项可见性过滤 + 分隔符归一：visible 求值、连续分隔符合并、首尾分隔符裁剪。 */
  function visibleMenuItems(items: AppMenuItem[]): AppMenuItem[] {
    const out: AppMenuItem[] = []
    for (const it of items) {
      const visible = it.separator || !it.visible || it.visible()
      if (!visible) continue
      if (it.separator && (out.length === 0 || out[out.length - 1].separator)) continue
      out.push(it)
    }
    while (out.length && out[out.length - 1].separator) out.pop()
    return out
  }

  // 执行菜单项动作
  function runItem(item: AppMenuItem) {
    if (item.disabled) return
    if (item.onClick) {
      item.onClick()
    } else if (item.link) {
      // 若 link 是某非隐藏 entry activity 的 route，用 openApp（加入任务栏 + 聚焦）；
      // 否则 navigateMain（深链接，如 /article/xxx 或 hiddenFromNav activity 的 route）。
      if (appManager.isEntryRouteVisible(item.link)) {
        navController.openApp(item.link)
      } else {
        navController.navigateMain(item.link)
      }
    }
  }
</script>

{#if systemMenus.length > 0}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger
      class="hover:bg-accent flex size-9 shrink-0 items-center justify-center rounded-md transition-colors"
      aria-label="系统菜单"
      title="系统菜单"
    >
      <img src={logoUrl} alt="GaubeeOS" class="size-6 rounded-md" />
    </DropdownMenu.Trigger>
    <DropdownMenu.Content align="start" side="top">
      <!-- 各应用注册的 system 菜单拍平为一列（组间自动分隔线），统一做可见性过滤与分隔符归一 -->
      {@const items = systemMenus.flatMap((m) => m.items ?? [])}
      {@const visibleItems = visibleMenuItems(items)}
      {#each visibleItems as item, ii (item.id)}
        {#if item.separator}
          {#if ii > 0 && ii < visibleItems.length - 1}
            <DropdownMenu.Separator />
          {/if}
        {:else}
          <DropdownMenu.Item onclick={() => runItem(item)} disabled={item.disabled}>
            {#if item.icon}
              {@const Icon = item.icon}
              <Icon class="size-4" />
            {/if}
            <span>{item.title}</span>
          </DropdownMenu.Item>
        {/if}
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/if}
