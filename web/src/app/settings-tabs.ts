export const settingsTabs = {
  general: "General",
  containers: "Containers and groups",
  bookmarks: "Bookmarks",
  widgets: "Widgets",
  notifications: "Notifications",
  appearance: "Appearance",
  history: "History and data",
  config: "Config file",
  security: "Security",
  boards: "Boards",
  integrations: "Integrations",
  users: "Users",
} as const;

export type SettingsTab = keyof typeof settingsTabs;

export function isSettingsTab(value: string): value is SettingsTab {
  return Object.hasOwn(settingsTabs, value);
}
