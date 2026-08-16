<!--
	ManagerOnlyGuard：managerOnly 应用的深链/权限占位视图。

	未登录 → 引导去账户页登录；已登录非管理员 → 提示权限不足。
	（真正的安全边界在后端 API 门禁；本视图是 UX 层。）
-->
<script lang="ts">
  import { backendSession } from '$lib/auth/backend-session.svelte'
  import { isAuthConfigured } from '$lib/auth/session.svelte'
  import { navController } from '$lib/nav/nav-controller-instance'
  import { Button } from '$lib/components/ui/button'
  import LockIcon from '@lucide/svelte/icons/lock'
  import UserRoundIcon from '@lucide/svelte/icons/user-round'

  function goLogin(): void {
    navController.navigateMain('/app/account')
  }
</script>

<div class="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
  <div class="bg-muted flex size-14 items-center justify-center rounded-full">
    <LockIcon class="text-muted-foreground size-6" />
  </div>
  <div class="text-lg font-semibold">此应用仅管理员可用</div>
  {#if backendSession.loaded && backendSession.authenticated}
    <div class="text-muted-foreground max-w-sm text-sm">
      当前登录为 <span class="text-foreground font-medium">{backendSession.login}</span>，
      不是本站管理员，无权访问管理类应用。
    </div>
  {:else}
    <div class="text-muted-foreground max-w-sm text-sm">
      {#if isAuthConfigured}
        登录管理员 GitHub 账户后可访问。
      {:else}
        此部署未配置 OAuth（VITE_AUTH_BASE），管理员功能不可用。
      {/if}
    </div>
  {/if}
  {#if isAuthConfigured}
    <Button variant="outline" onclick={goLogin}>
      <UserRoundIcon data-icon="inline-start" />
      {backendSession.authenticated ? '切换账户' : '前往登录'}
    </Button>
  {/if}
</div>
