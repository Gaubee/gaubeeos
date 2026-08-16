<!--
	SeoRouteBridge：路由级 SEO 的静态默认值注入桥（挂 ActivityRouter outlet 内）。

	active=false（后台保活 shell）时跳过——仅激活应用驱动 SEO；
	组件内（如 ArticleDetailView）后续 $effect 的 setSEO 覆盖动态值
	（effect 顺序：Bridge 先设默认，子组件后覆盖）。
-->
<script lang="ts">
  import { useRoute } from '$lib/router'
  import { seoStore } from '$lib/seo/head.svelte'

  let { active = true }: { active?: boolean } = $props()

  const route = useRoute()

  $effect(() => {
    if (!active) return
    const leaf = route?.()
    seoStore.setRouteDefaults(leaf?.route.seo ?? {})
  })
</script>
