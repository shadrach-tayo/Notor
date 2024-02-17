import {
  BaseQueryFn,
  FetchArgs,
  FetchBaseQueryError,
  createApi,
  fetchBaseQuery,
  retry,
} from "@reduxjs/toolkit/query/react";
import { tags } from "./tags";
import { logout, setToken } from "@/slices/authSlice";
import { RootState } from "@/store/store";

const baseQueryWithRetry = (baseUrl: string) =>
  retry(
    fetchBaseQuery({
      baseUrl,
      mode: "cors",
      prepareHeaders(headers, api) {
        const accessToken = (api.getState() as RootState).auth.tokens.google
          ?.access_token;

        if (accessToken) {
          // console.log("accessToken", accessToken);
          headers.set("authorization", `Bearer ${accessToken}`);
        }

        return headers;
      },
    }),
    {
      maxRetries: 3,
    }
  );

const baseQueryWithRefresh: BaseQueryFn<
  string | FetchArgs,
  unknown,
  FetchBaseQueryError
> = async (args, api, extraOptions) => {
  const baseQuery = baseQueryWithRetry("https://www.googleapis.com");
  let result = await baseQuery(args, api, extraOptions);
  if (result.error && result.error.status === 401) {
    console.log("API ERROR", result);
    const refreshResult = await (
      await fetch("http://localhost:4875/api/google_auth/refresh", {
        method: "POST",
      })
    ).json();
    console.log("refresh data", refreshResult);
    if (refreshResult && refreshResult.access_token) {
      api.dispatch(setToken(refreshResult));
    } else {
      api.dispatch(logout());
    }
  }
  return result;
};

export const googleApi = createApi({
  reducerPath: "api",
  baseQuery: baseQueryWithRefresh,
  tagTypes: [tags.user, tags.calendarList, tags.events],
  endpoints: () => ({}),
});
