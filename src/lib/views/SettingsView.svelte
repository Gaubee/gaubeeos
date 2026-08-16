<!--
	SettingsView：macOS 式系统设置（双栏 + 深链子页，2026-08-16 升级）。

	路由：/app/settings（默认面板 = 首个 system section）+ /app/settings/:section。
	布局：桌面（md+）左 sidebar（系统组 + 应用组 + 应用市场）/ 右 pane；
	      移动端同路由响应式——无 :section 显列表，有 :section 显 pane + 返回。

	解耦设计：面板通过 settingsSectionsRegistry 声明式注册（AppManager 安装/卸载联动），
	render 型自动获得 /app/settings/{id} 子页；link 型点击直接跳目标应用路由。
-->
<script lang="ts">
  import { settingsSectionsRegistry, type SettingsSection } from '$lib/apps/builtin/settings-sections'
  import { navController } from '$lib/nav/nav-controller-instance'
  import { useParams } from '$lib/router'
  import { Button } from '$lib/components/ui/button'
  import StoreIcon from '@lucide/svelte/icons/store'
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right'
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left'
  import SettingsIcon from '@lucide/svelte/icons/settings'
  import CircleAlertIcon from '@lucide/svelte/icons/circle-alert'

  const params = useParams<{ section: string }>()
  const sectionId = $derived(params?.()?.section)

  const systemSections = $derived(settingsSectionsRegistry.forGroup('system'))
  const appSections = $derived(settingsSectionsRegistry.forGroup('app'))
  /** render 型面板（可成为 :section 子页）；link 型不进 pane 选择。 */
  const paneSections = $derived([...systemSections, ...appSections].filter((s) => s.render))
  const current = $derived(
    sectionId ? settingsSectionsRegistry.get(sectionId) : paneSections[0],
  )
  const activeId = $derived(current?.id ?? '')

  function openSection(section: SettingsSection) {
    if (section.link) {
      navController.navigateMain(section.link)
    } else if (section.render) {
      navController.navigateMain(`/app/settings/${section.id}`)
    }
  }

  function backToList() {
    navController.navigateMain('/app/settings')
  }
</script>

<div class="mx-auto flex h-full max-w-4xl flex-col p-4 md:p-6">
  <h1 class="mb-4 flex items-center gap-2 text-2xl font-semibold">
    {#if sectionId}
      <!-- 移动端 pane 内的返回（桌面端 sidebar 常驻，隐藏） -->
      <Button variant="ghost" size="icon" class="md:hidden" onclick={backToList} aria-label="返回设置列表">
        <ArrowLeftIcon class="size-5" />
      </Button>
    {/if}
    设置
  </h1>

  <div class="grid min-h-0 flex-1 gap-4 md:grid-cols-[220px_minmax(0,1fr)]">
    <!-- ============ 左：sidebar（桌面常驻；移动端仅列表态显示） ============ -->
    <nav
      class="bg-card min-h-0 overflow-y-auto rounded-lg border border-border p-2 scrollbar-thin {sectionId ? 'hidden md:block' : ''}"
      aria-label="设置导航"
    >
      <div class="text-muted-foreground px-2 pt-1 pb-2 text-xs font-medium tracking-wide">系统</div>
      {#each systemSections as section (section.id)}
        <button
          class="hover:bg-accent flex w-full items-center gap-2.5 rounded-md px-2 py-2 text-left transition-colors {activeId === section.id ? 'bg-accent' : ''}"
          onclick={() => openSection(section)}
        >
          {#if section.icon}
            <!-- svelte-ignore ownership_invalid_mutation -->
            <section.icon class="text-muted-foreground size-4 shrink-0" />
          {/if}
          <span class="truncate text-sm">{section.title}</span>
        </button>
      {/each}

      <div class="text-muted-foreground px-2 pt-4 pb-2 text-xs font-medium tracking-wide">应用</div>
      {#each appSections as section (section.id)}
        <button
          class="hover:bg-accent flex w-full items-center gap-2.5 rounded-md px-2 py-2 text-left transition-colors {activeId === section.id ? 'bg-accent' : ''}"
          onclick={() => openSection(section)}
        >
          {#if section.icon}
            <!-- svelte-ignore ownership_invalid_mutation -->
            <section.icon class="text-muted-foreground size-4 shrink-0" />
          {/if}
          <!-- 侧边栏显示应用名（如「文章」「账户」）；面板标题保留具体设置名（如「文章源」） -->
          <span class="truncate text-sm">{section.app ?? section.title}</span>
        </button>
      {/each}
      {#if appSections.length === 0}
        <div class="text-muted-foreground px-2 py-2 text-xs">已安装应用暂无可配置项</div>
      {/if}

      <div class="bg-border my-2 h-px" role="separator"></div>
      <button
        class="hover:bg-accent flex w-full items-center gap-2.5 rounded-md px-2 py-2 text-left transition-colors"
        onclick={() => navController.navigateMain('/app/store')}
      >
        <StoreIcon class="text-muted-foreground size-4 shrink-0" />
        <span class="text-sm">应用市场</span>
      </button>
    </nav>

    <!-- ============ 右：pane（桌面常驻；移动端仅子页态显示） ============ -->
    <div class="min-h-0 {sectionId ? '' : 'hidden md:block'}">
      {#if current?.render}
        <div class="bg-card h-full overflow-y-auto rounded-lg border border-border p-4 scrollbar-thin">
          <div class="mb-4 flex items-center gap-2.5">
            {#if current.icon}
              <!-- svelte-ignore ownership_invalid_mutation -->
              <current.icon class="text-muted-foreground size-5" />
            {/if}
            <div>
              <div class="font-semibold">{current.title}</div>
              {#if current.description}
                <div class="text-muted-foreground text-xs">{current.description}</div>
              {/if}
            </div>
          </div>
          <current.render />
        </div>
      {:else if current}
        <!-- link 型被直接导航（理论上 openSection 已拦截），渲染兜底入口 -->
        <div class="bg-card h-full rounded-lg border border-border p-4">
          <Button onclick={() => openSection(current)}>打开{current.title}</Button>
        </div>
      {:else}
        <!-- 无任何面板 / 未知 section id -->
        <div class="bg-card flex h-full flex-col items-center justify-center gap-3 rounded-lg border border-border p-8 text-center">
          {#if sectionId}
            <CircleAlertIcon class="text-muted-foreground size-8" />
            <div class="text-sm">未知的设置面板：<code class="font-mono text-xs">{sectionId}</code></div>
            <Button variant="outline" size="sm" onclick={backToList}>
              <ArrowLeftIcon data-icon="inline-start" />
              返回设置
            </Button>
          {:else}
            <SettingsIcon class="text-muted-foreground size-8" />
            <div class="text-muted-foreground text-sm">暂无可配置项</div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>
