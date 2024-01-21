import { PayloadAction, createSlice } from "@reduxjs/toolkit";

type AuthTokensMap = {
  google?: GoogleAuthToken;
};

export type GoogleAuthToken = {
  access_token: string;
  token_type: string;
  expires_in: number;
  refresh_token: string;
  scope: string;
  expires_at: number;
};

export type UserInfo = {
  id: number;
  email: string;
  verified_email: boolean;
  name: string;
  given_name: string;
  family_name: string;
  picture: string;
  locale: string;
};

type AuthState = {
  tokens: AuthTokensMap;
  // googleAuthToken?: GoogleAuthToken;
  user: UserInfo;
};

const initialState: AuthState = {
  tokens: {},
  user: {
    id: 0,
    email: "",
    name: "",
    picture: "",
    given_name: "",
    verified_email: false,
    locale: "en",
    family_name: "",
  },
};

const authSlice = createSlice({
  name: "auth",
  initialState,
  reducers: {
    setUser: (state, { payload }: PayloadAction<AuthState["user"]>) => {
      state.user = payload;
    },
    setToken: (
      state,
      {
        payload,
      }: PayloadAction<{
        provider: keyof AuthTokensMap;
        token: AuthTokensMap[keyof AuthTokensMap];
      }>
    ) => {
      state.tokens[payload.provider] = payload.token;
    },
    removeToken: (
      state,
      {
        payload,
      }: PayloadAction<{
        provider: keyof AuthTokensMap;
      }>
    ) => {
      delete state.tokens[payload.provider];
    },
    logout: () => {
      return initialState;
    },
  },
});

export default authSlice.reducer;
export const { setUser, logout, setToken, removeToken } = authSlice.actions;
