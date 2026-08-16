<!--
	StatusBarSection：底部状态栏外链配置（系统设置的 system 组面板）。

	管理 siteStore.footerLinks（后端 config.toml [site] 段，全站生效）：
	增删改 + 上移/下移 + 保存。备案号合规提示：url 须为 beian.miit.gov.cn，
	新窗口 + noopener 由 FooterBar 渲染层统一保证。
-->
<script lang="ts">
  import { siteStore, type FooterLink } from '$lib/site/site-store.svelte'
  import { notifyError, notifySuccess } from '$lib/apps/builtin/notifications/service.svelte'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import PlusIcon from '@lucide/svelte/icons/plus'
  import Trash2Icon from '@lucide/svelte/icons/trash-2'
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up'
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down'
  import { onMount } from 'svelte'

  onMount(() => {
    if (!siteStore.loaded) void siteStore.load()
  })

  /** 编辑副本（保存成功才落 store）。 */
  let draft = $state<FooterLink[]>([])
  let dirty = $state(false)
  let saving = $state(false)

  // 外部状态（后端装载）变化时重置草稿
  let lastSynced = $state('')
  $effect(() => {
    const json = JSON.stringify(siteStore.footerLinks)
    if (json !== lastSynced) {
      lastSynced = json
      draft = siteStore.footerLinks.map((l) => ({ ...l }))
      dirty = false
    }
  })

  function markDirty(): void {
    dirty = true
  }

  function addLink(): void {
    draft = [...draft, { id: `link-${Date.now()}`, label: '', url: '' }]
    dirty = true
  }

  function removeLink(i: number): void {
    draft = draft.filter((_, idx) => idx !== i)
    dirty = true
  }

  function move(i: number, delta: -1 | 1): void {
    const j = i + delta
    if (j < 0 || j >= draft.length) return
    const next = [...draft]
    ;[next[i], next[j]] = [next[j], next[i]]
    draft = next
    dirty = true
  }

  async function handleSave(): Promise<void> {
    const cleaned = draft
      .map((l) => ({ ...l, label: l.label.trim(), url: l.url.trim() }))
      .filter((l) => l.label && l.url)
    if (cleaned.length !== draft.length) {
      notifyError('存在空的 label / url 条目，请补全或删除')
      return
    }
    if (cleaned.some((l) => !(l.url.startsWith('http://') || l.url.startsWith('https://')))) {
      notifyError('url 必须以 http:// 或 https:// 开头')
      return
    }
    saving = true
    try {
      await siteStore.save(cleaned)
      notifySuccess('状态栏外链已保存')
    } catch (e) {
      notifyError(e instanceof Error ? e.message : String(e))
    } finally {
      saving = false
    }
  }
</script>

<div class="space-y-3">
  {#if siteStore.error && !siteStore.loaded}
    <div class="text-destructive bg-destructive/10 rounded-md px-3 py-2 text-xs">
      站点配置后端不可达：{siteStore.error}（当前显示回退默认；保存需 static-server 在线）
    </div>
  {/if}

  {#if draft.length === 0}
    <div class="text-muted-foreground px-1 py-2 text-xs">
      尚未配置任何外链。常见：GitHub 源码入口、ICP 备案号（url 填
      <code class="font-mono">https://beian.miit.gov.cn/</code>，label 填备案号）。
    </div>
  {:else}
    <div class="space-y-2">
      {#each draft as link, i (link.id)}
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
            <Button variant="outline" size="sm" onclick={() => move(i, 1)} disabled={i === draft.length - 1} aria-label="下移">
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

  <div class="flex items-center gap-2">
    <Button variant="outline" size="sm" onclick={addLink}>
      <PlusIcon data-icon="inline-start" />
      添加外链
    </Button>
    <Button size="sm" class="ml-auto" onclick={handleSave} disabled={saving || (!dirty && siteStore.loaded)}>
      {saving ? '保存中…' : dirty ? '保存' : '已保存'}
    </Button>
  </div>

  <p class="text-muted-foreground text-[11px]">
    外链对新窗口打开并带 noopener。中国大陆备案站点：请添加一条 label 为备案号、url 为
    <code class="font-mono">https://beian.miit.gov.cn/</code> 的链接以满足合规展示。
  </p>
</div>
