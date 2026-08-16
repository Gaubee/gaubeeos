/**
 * 设置面板注册表（macOS 式系统设置模型，2026-08-16 升级）。
 *
 * 解决「设置页硬编码各功能面板」的刚性耦合：
 * 应用通过 manifest.settingsSections 声明自己的设置面板，
 * AppManager 在安装/卸载时投影到本注册表，SettingsView 动态渲染。
 *
 * 两种入口形态：
 * - link：点击导航到指定深链接（如 /app/account），由目标应用自己提供完整界面。
 * - render：内联渲染一个 Svelte 组件（获得 /app/settings/{id} 深链子页）。
 *
 * 两种分组（group，参考 macOS 系统设置的「系统 / 应用」分区）：
 * - system：系统级偏好（外观/状态栏/关于），由设置应用自己注册。
 * - app：应用自己的设置（如文章源/说说源），由各应用注册，默认值。
 *
 * 谁提供能力，谁注册入口；设置应用本身不反向依赖具体业务应用。
 */
import type { Component } from "svelte";

/** 面板分组：system = 系统偏好区，app = 应用设置区。 */
export type SettingsSectionGroup = "system" | "app";

/** 一个设置面板入口声明。 */
export interface SettingsSection {
  /** 唯一标识（如 'articles.sources'、'about'；render 型会获得 /app/settings/{id} 子页）。 */
  id: string;
  /** 显示标题。 */
  title: string;
  /** 描述（副标题）。 */
  description?: string;
  /** Lucide 图标组件。 */
  icon?: Component;
  /** 分组（默认 'app'）。 */
  group?: SettingsSectionGroup;
  /** 声明方应用的显示名（AppManager 投影时注入；侧边栏应用组用它作条目标签）。 */
  app?: string;
  /** 组内渲染顺序，越小越靠前（默认按注册顺序）。 */
  order?: number;
  /** 点击后导航到的深链接路径（优先于 render，不产生设置内子页）。 */
  link?: string;
  /** 内联渲染的面板内容组件（link 未提供时使用）。 */
  render?: Component;
}

class SettingsSectionRegistry {
  private readonly sections = new Map<string, SettingsSection>();

  /** 注册或覆盖一个设置面板。 */
  register(section: SettingsSection): void {
    this.sections.set(section.id, section);
  }

  /** 注销一个设置面板。 */
  unregister(id: string): void {
    this.sections.delete(id);
  }

  /** 是否已注册某面板。 */
  has(id: string): boolean {
    return this.sections.has(id);
  }

  /** 按 id 取面板声明。 */
  get(id: string): SettingsSection | undefined {
    return this.sections.get(id);
  }

  /** 全部面板，按 order（升序）→ 注册顺序稳定排序。 */
  all(): SettingsSection[] {
    return [...this.sections.values()].sort((a, b) => {
      const oa = a.order ?? Number.MAX_SAFE_INTEGER;
      const ob = b.order ?? Number.MAX_SAFE_INTEGER;
      return oa - ob;
    });
  }

  /** 按分组过滤（组内仍按 order 排序）。 */
  forGroup(group: SettingsSectionGroup): SettingsSection[] {
    return this.all().filter((s) => (s.group ?? "app") === group);
  }
}

/** 全局设置面板注册表单例。 */
export const settingsSectionsRegistry = new SettingsSectionRegistry();
