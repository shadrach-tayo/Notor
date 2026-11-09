export type Preferences = {
  storage_path: String;
  notify_only_meetings: boolean;
  accounts_preferences: { [key: string]: AccountPreference };
};

export type AccountPreference = {
  hidden_calendars: string[];
};

/** Health of one signed-in Google account, as shown in preferences. */
export type AccountHealth = {
  email: string;
  disabled?: boolean;
  /** Saved across restarts. The account is not polled until the user signs in again. */
  needs_signin?: boolean;
};
