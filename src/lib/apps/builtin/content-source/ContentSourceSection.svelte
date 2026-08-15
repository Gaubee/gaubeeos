<!--
	ContentSourceSection：内容源订阅管理面板（设置的 render 型 section）。

	职责（正交意图）：
	1. 订阅列表：源卡片（启停 Switch / 立即同步 / 编辑 / 删除 + lastSync/错误状态）。
	2. 新增/编辑表单（Dialog）：owner/repo/ref/collection/include/interval/slugPrefix，
	   带「测试连接」（glob 命中预览，不下载）。
	3. 变更联动：CRUD/同步完成后触发 contentQuery.refreshFromRemote()（列表/搜索即时刷新）。

	数据后端：/api/sources（见 $lib/content-source），dev 经 vite proxy → :8090。
-->
<script lang="ts">
  import { contentQuery } from '$lib/content-pipeline/query.svelte'
  import { contentSourceStore } from '$lib/content-source/store.svelte'
  import * as api from '$lib/content-source/client'
  import type { Collection, SourceWithState, TestResult } from '$lib/content-source/types'
  import { notifyError, notifySuccess } from '$lib/apps/builtin/notifications/service.svelte'
  import * as Dialog from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'
  import { Badge } from '$lib/components/ui/badge'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Switch } from '$lib/components/ui/switch'
  import * as Select from '$lib/components/ui/select'
  import { Separator } from '$lib/components/ui/separator'
  import PlusIcon from '@lucide/svelte/icons/plus'
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw'
  import PencilIcon from '@lucide/svelte/icons/pencil'
  import Trash2Icon from '@lucide/svelte/icons/trash-2'
  import FlaskConicalIcon from '@lucide/svelte/icons/flask-conical'
  import { onMount } from 'svelte'

  onMount(() => {
    void contentSourceStore.ensureLoaded()
  })

  const sources = $derived(contentSourceStore.sources)
  const status = $derived(contentSourceStore.status)

  // ---- 表单状态 ----
  let dialogOpen = $state(false)
  /** 正在编辑的源 id（null = 新增）。 */
  let editingId = $state<string | null>(null)
  let form = $state({
    name: '',
    owner: '',
    repo: '',
    ref: '',
    collection: 'articles' as Collection,
    include: 'src/content/articles/**/*.md',
    slugPrefix: '',
    interval: '1h',
  })
  let testing = $state(false)
  let testResult = $state<TestResult | null>(null)
  let testError = $state<string | null>(null)
  let saving = $state(false)
  /** 同步中的源 id 集合（按钮 loading 态）。 */
  let syncingIds = $state<Set<string>>(new Set())

  function openCreate(): void {
    editingId = null
    form = {
      name: '',
      owner: '',
      repo: '',
      ref: '',
      collection: 'articles',
      include: 'src/content/articles/**/*.md',
      slugPrefix: '',
      interval: '1h',
    }
    testResult = null
    testError = null
    dialogOpen = true
  }

  function openEdit(s: SourceWithState): void {
    editingId = s.id
    form = {
      name: s.name ?? '',
      owner: s.owner,
      repo: s.repo,
      ref: s.ref,
      collection: s.collection,
      include: s.include,
      slugPrefix: s.slug_prefix ?? '',
      interval: s.interval,
    }
    testResult = null
    testError = null
    dialogOpen = true
  }

  function onCollectionChange(v: Collection): void {
    form.collection = v
    // 便捷默认 include（未自定义时跟随切换）
    if (
      form.include === 'src/content/articles/**/*.md' ||
      form.include === 'src/content/events/**/*.md'
    ) {
      form.include = `src/content/${v}/**/*.md`
    }
  }

  async function handleTest(): Promise<void> {
    if (!form.owner.trim() || !form.repo.trim() || !form.include.trim()) {
      testError = 'owner / repo / include 不能为空'
      return
    }
    testing = true
    testResult = null
    testError = null
    try {
      testResult = await api.testConnection({
        owner: form.owner.trim(),
        repo: form.repo.trim(),
        ref: form.ref.trim(),
        include: form.include.trim(),
      })
    } catch (e) {
      testError = e instanceof Error ? e.message : String(e)
    } finally {
      testing = false
    }
  }

  async function handleSave(): Promise<void> {
    if (!form.owner.trim() || !form.repo.trim() || !form.include.trim()) {
      notifyError('owner / repo / include 不能为空')
      return
    }
    saving = true
    try {
      const input = {
        name: form.name.trim() || undefined,
        owner: form.owner.trim(),
        repo: form.repo.trim(),
        ref: form.ref.trim(),
        collection: form.collection,
        include: form.include.trim(),
        slug_prefix: form.slugPrefix.trim() || undefined,
        interval: form.interval,
      }
      if (editingId) {
        await contentSourceStore.update(editingId, input)
        notifySuccess('订阅已更新，正在重新同步')
      } else {
        const r = await contentSourceStore.create(input)
        notifySuccess(r.outcome?.error ? `订阅已创建，但同步失败：${r.outcome.error}` : '订阅已创建并完成首轮同步')
      }
      dialogOpen = false
      await contentQuery.refreshFromRemote()
    } catch (e) {
      notifyError(e instanceof Error ? e.message : String(e))
    } finally {
      saving = false
    }
  }

  async function handleSync(s: SourceWithState): Promise<void> {
    syncingIds = new Set([...syncingIds, s.id])
    try {
      const outcome = await contentSourceStore.syncNow(s.id)
      if (outcome.error) notifyError(`同步失败：${outcome.error}`)
      else notifySuccess(outcome.skipped ? '无变化（已是最新）' : `已同步：下载 ${outcome.downloaded}，移除 ${outcome.removed}，共 ${outcome.total_files} 篇`)
      await contentQuery.refreshFromRemote()
    } catch (e) {
      notifyError(e instanceof Error ? e.message : String(e))
    } finally {
      const next = new Set(syncingIds)
      next.delete(s.id)
      syncingIds = next
    }
  }

  async function handleToggle(s: SourceWithState, enabled: boolean): Promise<void> {
    try {
      await contentSourceStore.setEnabled(s.id, enabled)
      await contentQuery.refreshFromRemote()
    } catch (e) {
      notifyError(e instanceof Error ? e.message : String(e))
    }
  }

  async function handleDelete(s: SourceWithState): Promise<void> {
    if (!confirm(`删除订阅「${s.display_name}」？缓存内容将一并清除。`)) return
    try {
      await contentSourceStore.remove(s.id)
      notifySuccess('订阅已删除')
      await contentQuery.refreshFromRemote()
    } catch (e) {
      notifyError(e instanceof Error ? e.message : String(e))
    }
  }

  function fmtTime(iso?: string): string {
    if (!iso) return '从未同步'
    const d = new Date(iso)
    return d.toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })
  }
</script>

<div class="space-y-3">
  {#if status === 'error'}
    <div class="text-destructive bg-destructive/10 rounded-md px-3 py-2 text-xs">
      订阅引擎不可达：{contentSourceStore.error}（请确认 static-server 已启动）
    </div>
  {/if}

  {#if sources.length === 0}
    <div class="text-muted-foreground px-1 py-2 text-xs">
      尚未订阅任何内容源。添加一个 GitHub 仓库作为文章/说说来源。
    </div>
  {:else}
    <div class="space-y-2">
      {#each sources as s (s.id)}
        <div class="rounded-lg border border-border p-3">
          <div class="flex items-center gap-2">
            <Switch checked={s.enabled} onCheckedChange={(v) => handleToggle(s, v)} aria-label="启停订阅" />
            <span class="text-sm font-medium">{s.display_name}</span>
            <Badge variant="outline" class="text-[10px]">
              {s.collection === 'articles' ? '文章' : '说说'}
            </Badge>
            {#if s.state.last_error}
              <Badge variant="destructive" class="text-[10px]">同步出错</Badge>
            {/if}
            <span class="text-muted-foreground ml-auto text-[11px]">
              {s.state.entries?.length ?? 0} 篇 · {fmtTime(s.state.last_sync_at)}
            </span>
          </div>
          <div class="text-muted-foreground mt-1.5 truncate font-mono text-[11px]">
            {s.owner}/{s.repo}{s.ref ? `@${s.ref}` : ''} · {s.include}
          </div>
          {#if s.state.last_error}
            <div class="text-destructive mt-1 truncate text-[11px]" title={s.state.last_error}>
              {s.state.last_error}
            </div>
          {/if}
          <div class="mt-2 flex items-center gap-1.5">
            <Button variant="outline" size="sm" onclick={() => handleSync(s)} disabled={syncingIds.has(s.id) || !s.enabled}>
              <RefreshCwIcon data-icon="inline-start" class={syncingIds.has(s.id) ? 'animate-spin' : ''} />
              同步
            </Button>
            <Button variant="outline" size="sm" onclick={() => openEdit(s)}>
              <PencilIcon data-icon="inline-start" />
              编辑
            </Button>
            <Button variant="outline" size="sm" class="text-destructive" onclick={() => handleDelete(s)}>
              <Trash2Icon data-icon="inline-start" />
              删除
            </Button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <Button size="sm" onclick={openCreate}>
    <PlusIcon data-icon="inline-start" />
    添加内容源
  </Button>

  <Dialog.Root bind:open={dialogOpen}>
    <Dialog.Content class="max-h-[85vh] overflow-y-auto sm:max-w-lg">
      <Dialog.Header>
        <Dialog.Title>{editingId ? '编辑内容源' : '添加内容源'}</Dialog.Title>
        <Dialog.Description>
          订阅一个 GitHub 仓库的 markdown 内容，按路径匹配映射为本地文章/说说。
        </Dialog.Description>
      </Dialog.Header>

      <div class="grid gap-3 py-2">
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-1.5">
            <Label for="src-owner">owner</Label>
            <Input id="src-owner" bind:value={form.owner} placeholder="gaubee" />
          </div>
          <div class="grid gap-1.5">
            <Label for="src-repo">repo</Label>
            <Input id="src-repo" bind:value={form.repo} placeholder="gaubee.com" />
          </div>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-1.5">
            <Label for="src-ref">ref（可选，默认分支）</Label>
            <Input id="src-ref" bind:value={form.ref} placeholder="main" />
          </div>
          <div class="grid gap-1.5">
            <Label>内容类型</Label>
            <Select.Root type="single" value={form.collection} onValueChange={(v) => v && onCollectionChange(v as Collection)}>
              <Select.Trigger class="w-full">{form.collection === 'articles' ? '文章' : '说说'}</Select.Trigger>
              <Select.Content>
                <Select.Item value="articles" label="文章" />
                <Select.Item value="events" label="说说" />
              </Select.Content>
            </Select.Root>
          </div>
        </div>
        <div class="grid gap-1.5">
          <Label for="src-include">include（路径匹配 glob）</Label>
          <Input id="src-include" bind:value={form.include} placeholder="src/content/articles/**/*.md" class="font-mono text-xs" />
          <span class="text-muted-foreground text-[11px]">
            匹配的文件路径（仓库相对）→ 本地 URL：/article/{form.collection}/{'{slug 前缀}'}{form.collection === 'articles' ? '0063.my-post' : '00001.my-event'}
          </span>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-1.5">
            <Label for="src-interval">同步频率</Label>
            <Select.Root type="single" value={form.interval} onValueChange={(v) => v && (form.interval = v)}>
              <Select.Trigger class="w-full">{form.interval}</Select.Trigger>
              <Select.Content>
                {#each ['15m', '30m', '1h', '6h', '12h', '24h'] as it}
                  <Select.Item value={it} label={it} />
                {/each}
              </Select.Content>
            </Select.Root>
          </div>
          <div class="grid gap-1.5">
            <Label for="src-prefix">slug 前缀（可选）</Label>
            <Input id="src-prefix" bind:value={form.slugPrefix} placeholder="blog-" />
          </div>
        </div>
        <div class="grid gap-1.5">
          <Label for="src-name">展示名（可选）</Label>
          <Input id="src-name" bind:value={form.name} placeholder="我的博客" />
        </div>

        <Separator />

        <div class="flex items-center gap-2">
          <Button variant="outline" size="sm" onclick={handleTest} disabled={testing}>
            <FlaskConicalIcon data-icon="inline-start" class={testing ? 'animate-pulse' : ''} />
            测试连接
          </Button>
          {#if testResult}
            <span class="text-xs">命中 {testResult.matched} 个 .md（@{testResult.resolved_ref.slice(0, 8)}）</span>
          {:else if testError}
            <span class="text-destructive truncate text-xs" title={testError}>{testError}</span>
          {/if}
        </div>
        {#if testResult?.sample.length}
          <div class="bg-muted rounded-md p-2 font-mono text-[10px] leading-relaxed">
            {#each testResult.sample.slice(0, 5) as p}
              <div class="truncate">{p}</div>
            {/each}
            {#if testResult.matched > testResult.sample.length}
              <div class="text-muted-foreground">…共 {testResult.matched} 个</div>
            {/if}
          </div>
        {/if}
      </div>

      <Dialog.Footer>
        <Button variant="outline" onclick={() => (dialogOpen = false)}>取消</Button>
        <Button onclick={handleSave} disabled={saving}>
          {saving ? '保存中…' : editingId ? '保存并重新同步' : '订阅并同步'}
        </Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
</div>
