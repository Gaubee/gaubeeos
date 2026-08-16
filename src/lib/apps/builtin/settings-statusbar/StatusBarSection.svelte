<!--
	SiteSection：站点配置面板（系统设置的 system 组，深链 /app/settings/site）。

	两部分（事实源：后端 config.toml [site] 段，全站生效）：
	1. 站点信息（SEO）：站点名 / 描述 / base_url（启用 canonical/sitemap/og:url）
	   / og_image / 允许索引（robots）。
	2. 底部状态栏外链：增删改 + 上移/下移。
	保存走 siteStore.save({ links, seo })。备案合规提示保留。
-->
<script lang="ts">
  import { siteStore, type FooterLink, type SiteSeo } from '$lib/site/site-store.svelte'
  import { notifyError, notifySuccess } from '$lib/apps/builtin/notifications/service.svelte'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Switch } from '$lib/components/ui/switch'
  import PlusIcon from '@lucide/svelte/icons/plus'
  import Trash2Icon from '@lucide/svelte/icons/trash-2'
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up'
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down'
  import { onMount } from 'svelte'

  onMount(() => {
    if (!siteStore.loaded) void siteStore.load()
  })

  let draftLinks = $state<FooterLink[]>([])
  let draftSeo = $state<SiteSeo>({
    site_name: 'GaubeeOS',
    description: null,
    base_url: null,
    og_image: null,
    allow_indexing: true,
  })
  let dirty = $state(false)
  let saving = $state(false)

  let lastSynced = $state('')
  $effect(() => {
    const json = JSON.stringify([siteStore.footerLinks, siteStore.site])
    if (json !== lastSynced) {
      lastSynced = json
      draftLinks = siteStore.footerLinks.map((l) => ({ ...l }))
      draftSeo = { ...siteStore.site }
      dirty = false
    }
  })

  function markDirty(): void {
    dirty = true
  }

  function addLink(): void {
    draftLinks = [...draftLinks, { id: `link-${Date.now()}`, label: '', url: '' }]
    dirty = true
  }

  function removeLink(i: number): void {
    draftLinks = draftLinks.filter((_, idx) => idx !== i)
    dirty = true
  }

  function move(i: number, delta: -1 | 1): void {
    const j = i + delta
    if (j < 0 || j >= draftLinks.length) return
    const next = [...draftLinks]
    ;[next[i], next[j]] = [next[j], next[i]]
    draftLinks = next
    dirty = true
  }

  async function handleSave(): Promise<void> {
    const cleaned = draftLinks
      .map((l) => ({ ...l, label: l.label.trim(), url: l.url.trim() }))
      .filter((l) => l.label && l.url)
    if (cleaned.length !== draftLinks.length) {
      notifyError('存在空的外链 label / url，请补全或删除')
      return
    }
    if (cleaned.some((l) => !(l.url.startsWith('http://') || l.url.startsWith('https://')))) {
      notifyError('外链 url 必须以 http:// 或 https:// 开头')
      return
    }
    const base = draftSeo.base_url?.trim() ?? ''
    if (base && !(base.startsWith('http://') || base.startsWith('https://'))) {
      notifyError('base_url 必须以 http:// 或 https:// 开头')
      return
    }
    if (base.endsWith('/')) {
      notifyError('base_url 不应以 / 结尾')
      return
    }
    saving = true
    try {
      await siteStore.save({ links: cleaned, seo: draftSeo })
      notifySuccess('站点配置已保存')
    } catch (e) {
      notifyError(e instanceof Error ? e.message : String(e))
    } finally {
      saving = false
    }
  }
</script>

<div class="space-y-5">
  {#if siteStore.error && !siteStore.loaded}
    <div class="text-destructive bg-destructive/10 rounded-md px-3 py-2 text-xs">
      站点配置后端不可达：{siteStore.error}（当前显示回退默认；保存需 static-server 在线）
    </div>
  {/if}

  <!-- ========== 站点信息（SEO） ========== -->
  <section class="space-y-3">
    <h3 class="font-medium text-sm">站点信息</h3>
    <div class="grid gap-3 sm:grid-cols-2">
      <div class="grid gap-1.5">
        <Label for="site-name" class="text-[11px]">站点名（title 后缀 / og:site_name）</Label>
        <Input id="site-name" bind:value={draftSeo.site_name} oninput={markDirty} placeholder="GaubeeOS" class="text-xs" />
      </div>
      <div class="grid gap-1.5">
        <Label for="site-base" class="text-[11px]">站点地址 base_url（启用 canonical/sitemap/og:url）</Label>
        <Input id="site-base" bind:value={draftSeo.base_url} oninput={markDirty} placeholder="https://example.com" class="font-mono text-xs" />
      </div>
    </div>
    <div class="grid gap-1.5">
      <Label for="site-desc" class="text-[11px]">站点描述（meta description / og:description 兜底）</Label>
      <Input id="site-desc" bind:value={draftSeo.description} oninput={markDirty} placeholder="一句话介绍你的站点" class="text-xs" />
    </div>
    <div class="grid gap-3 sm:grid-cols-2">
      <div class="grid gap-1.5">
        <Label for="site-og" class="text-[11px]">分享图 og:image（可选，绝对 URL）</Label>
        <Input id="site-og" bind:value={draftSeo.og_image} oninput={markDirty} placeholder="https://…/cover.png" class="font-mono text-xs" />
      </div>
      <div class="flex items-center justify-between rounded-lg border border-border px-3 py-2">
        <div>
          <div class="text-xs font-medium">允许搜索引擎索引</div>
          <div class="text-muted-foreground text-[11px]">关闭 → robots.txt 全站 Disallow + noindex</div>
        </div>
        <Switch
          checked={draftSeo.allow_indexing}
          onCheckedChange={(v) => { draftSeo.allow_indexing = v; markDirty() }}
          aria-label="允许搜索引擎索引"
        />
      </div>
    </div>
    <p class="text-muted-foreground text-[11px]">
      base_url 配置后：/robots.txt 带 Sitemap 行、/sitemap.xml 从订阅内容生成、文章页输出
      canonical 与 og:url。
    </p>
  </section>

  <div class="bg-border h-px" role="separator"></div>

  <!-- ========== 底部状态栏外链 ========== -->
  <section class="space-y-3">
    <h3 class="font-medium text-sm">底部状态栏外链</h3>
    {#if draftLinks.length === 0}
      <div class="text-muted-foreground px-1 py-2 text-xs">
        尚未配置任何外链。常见：GitHub 源码入口、ICP 备案号（url 填
        <code class="font-mono">https://beian.miit.gov.cn/</code>，label 填备案号）。
      </div>
    {:else}
      <div class="space-y-2">
        {#each draftLinks as link, i (link.id)}
          <div class="rounded-lg border border-border p-3">
            <div class="grid grid-cols-[minmax(0,2fr)_minmax(0,3fr)] gap-2">
              <div class="grid gap-1">
                <Label for={`fl-label-${i}`} class="text-[11px]">显示文字</Label>
                <Input
                  id={`fl-label-${i}`}
                  bind:value={link.label}
                  oninput={markDirty}
                  placeholder="GitHub / 闽ICP备xxxxxxxx号-x"
                  class="text-xs"
                />
              </div>
              <div class="grid gap-1">
                <Label for={`fl-url-${i}`} class="text-[11px]">链接（http(s)://）</Label>
                <Input
                  id={`fl-url-${i}`}
                  bind:value={link.url}
                  oninput={markDirty}
                  placeholder="https://…"
                  class="font-mono text-xs"
                />
              </div>
            </div>
            <div class="mt-2 flex items-center gap-1.5">
              <Button variant="outline" size="sm" onclick={() => move(i, -1)} disabled={i === 0} aria-label="上移">
                <ArrowUpIcon class="size-3.5" />
              </Button>
              <Button variant="outline" size="sm" onclick={() => move(i, 1)} disabled={i === draftLinks.length - 1} aria-label="下移">
                <ArrowDownIcon class="size-3.5" />
              </Button>
              <Button variant="outline" size="sm" class="text-destructive ml-auto" onclick={() => removeLink(i)}>
                <Trash2Icon data-icon="inline-start" />
                删除
              </Button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
    <Button variant="outline" size="sm" onclick={addLink}>
      <PlusIcon data-icon="inline-start" />
      添加外链
    </Button>
  </section>

  <div class="flex items-center gap-2">
    <Button size="sm" class="ml-auto" onclick={handleSave} disabled={saving || (!dirty && siteStore.loaded)}>
      {saving ? '保存中…' : dirty ? '保存' : '已保存'}
    </Button>
  </div>

  <p class="text-muted-foreground text-[11px]">
    外链对新窗口打开并带 noopener。中国大陆备案站点：请添加一条 label 为备案号、url 为
    <code class="font-mono">https://beian.miit.gov.cn/</code> 的链接以满足合规展示。
  </p>
</div>
