import {
  BaseQueryFn,
  FetchArgs,
  FetchBaseQueryError,
  createApi,
  fetchBaseQuery,
  retry,
} from "@reduxjs/toolkit/query/react";
import { tags } from "./tags";

const baseQueryWithRetry = (baseUrl: string) =>
  retry(
    fetchBaseQuery({
      baseUrl,
      mode: "cors",
      prepareHeaders(headers, _api) {
        return headers;
      },
    }),
    {
      maxRetries: 3,
    },
  );

const baseQueryWithRefresh: BaseQueryFn<
  string | FetchArgs,
  unknown,
  FetchBaseQueryError
> = async (args, api, extraOptions) => {
  const baseQuery = baseQueryWithRetry("https://www.googleapis.com");
  return baseQuery(args, api, extraOptions);
};

export const googleApi = createApi({
  reducerPath: "api",
  baseQuery: baseQueryWithRefresh,
  tagTypes: [tags.user, tags.calendarList, tags.events],
  endpoints: () => ({}),
});
