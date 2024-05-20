export type Preferences = {
  storage_path: String;
  notify_only_meetings: boolean;
  accounts_preferences: { [key: string]: AccountPreference };
};

export type AccountPreference = {
  hidden_calendars: string[];
};
