import type { ContentSource } from "../types";
/**
 * events 内容源（远程订阅模式）：由设置「内容源」配置的 GitHub 仓库聚合而来。
 *
 * 历史单体模式（gaubee.com）：构建期把 src/content/events 打进 bundle，
 * 见 readonlyVfs 路径（内核版已置空）。
 */
import { remoteCollectionSource } from "./remote";

export const COLLECTION = "events";

/** events 内容源单例。 */
export const eventsSource: ContentSource = remoteCollectionSource(COLLECTION);
