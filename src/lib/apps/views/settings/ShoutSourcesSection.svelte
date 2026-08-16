<!--
	ShoutSourcesSection：说说应用的设置面板（注册为 /app/settings/shout.sources）。
	wrapper 固定 collection，通用实现在 content-source/SourcesSection.svelte。
-->
<script lang="ts">
  import SourcesSection from '$lib/apps/builtin/content-source/SourcesSection.svelte'
</script>

<SourcesSection collection="events" />
