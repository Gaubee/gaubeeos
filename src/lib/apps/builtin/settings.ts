import AboutSection from "$lib/apps/views/AboutSection.svelte";
import { defineRoute } from "$lib/router";
import Info from "@lucide/svelte/icons/info";
import MoonIcon from "@lucide/svelte/icons/moon";
import PaletteIcon from "@lucide/svelte/icons/palette";
/**
 * 设置应用（系统内置，不可卸载）—— macOS 式系统设置。
 *
 * 路由（2026-08-16 升级）：/app/settings（默认面板）+ /app/settings/:section
 * （render 型面板深链子页，如 /app/settings/articles.sources）。
 * 面板由各应用经 manifest.settingsSections 声明式注册（AppManager 投影联动），
 * 本应用自身只注册 system 组：外观/状态栏/关于。
 */
import PanelBottomIcon from "@lucide/svelte/icons/panel-bottom";
import Settings from "@lucide/svelte/icons/settings";
import { toggleMode } from "mode-watcher";
import type { Component } from "svelte";
import { z } from "zod";

import type { AppEntry } from "../types";
import AppearanceSection from "./appearance/AppearanceSection.svelte";
import StatusBarSection from "./settings-statusbar/StatusBarSection.svelte";

export const settingsRoute = defineRoute({
  id: "settings",
  pattern: "",
  seo: { title: "设置" },
  component: () => import("$lib/views/SettingsView.svelte"),
  children: [
    defineRoute({
      id: "settings.section",
      pattern: ":section",
      seo: { title: "设置" },
      params: z.object({ section: z.string().min(1) }),
      component: () => import("$lib/views/SettingsView.svelte"),
    }),
  ],
});

export const settingsApp: AppEntry = {
  manifest: {
    id: "settings",
    name: "设置",
    icon: Settings,
    category: "system",
    defaultArea: "main",
    activities: [
      {
        pattern: "/app/settings",
        entry: true,
        root: settingsRoute,
      },
    ],
    vfsOwnership: [],
    settingsSections: [
      // ---- system 组：系统级偏好（设置应用自己注册）----
      {
        id: "appearance",
        title: "外观",
        description: "切换明暗主题",
        icon: PaletteIcon,
        group: "system",
        order: 10,
        render: AppearanceSection,
      },
      {
        id: "site",
        title: "站点",
        description: "站点信息（SEO）与底部状态栏外链",
        icon: PanelBottomIcon,
        group: "system",
        order: 20,
        render: StatusBarSection,
      },
      {
        id: "about",
        title: "关于",
        description: "系统信息",
        icon: Info,
        group: "system",
        order: 90,
        render: AboutSection as unknown as Component,
      },
    ],
    // 系统菜单（苹果菜单，LOGO 触发）：设置入口、主题切换、关于
    appMenus: [
      {
        id: "settings:main",
        title: "系统",
        placement: "system",
        order: 0,
        items: [
          { id: "settings-entry", title: "设置…", icon: Settings, link: "/app/settings" },
          { id: "theme-toggle", title: "切换主题", icon: MoonIcon, onClick: toggleMode },
          { id: "sep1", title: "-", separator: true },
          { id: "about", title: "关于 GaubeeOS", icon: Info, link: "/app/settings/about" },
        ],
      },
    ],
    description: "系统设置与偏好",
    longDescription:
      "macOS 式系统设置：系统级偏好（外观/状态栏/关于）+ 各应用声明式注册的设置面板（安装/卸载自动联动）。",
  },
};
