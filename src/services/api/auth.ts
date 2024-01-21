import { FetchBaseQueryError } from "@reduxjs/toolkit/query";
import { googleApi } from ".";
import { UserInfo, setUser } from "@/slices/authSlice";
import { tags } from "./tags";

export const authApi = googleApi.injectEndpoints({
  endpoints: (builder) => ({
    userInfo: builder.query<UserInfo | null, string>({
      providesTags: [{ type: tags.user }],
      query: (accessToken) => ({
        url: `oauth2/v2/userinfo?alt=json&access_token=${accessToken}`,
        method: "GET",
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
        },
      }),
      async onQueryStarted(arg, { dispatch, queryFulfilled }) {
        try {
          const res = await queryFulfilled;
          console.log("USER INFO", res.data);
          if (res.data) {
            dispatch(setUser(res.data));
          }
        } catch (err) {
          console.log("Error fetching user info", err);
        }
      },
    }),
  }),
});

export const { useUserInfoQuery } = authApi;
