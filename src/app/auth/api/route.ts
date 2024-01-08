import {
  GOOGLE_CLIENT_ID,
  GOOGLE_CLIENT_SECRET,
  GOOGLE_REDIRECT_URL,
} from "@/lib/config";
// import { cookies } from "next/headers";
import { google } from "googleapis";
import { NextRequest, NextResponse } from "next/server";
import { ParsedUrlQuery } from "querystring";
import url from "url";

export async function GET(request: NextRequest) {
  console.log("OAUTHCALLBACK");
  // const cookieStore = cookies();
  // const token = cookieStore.get("token");
  const searchParams = request.nextUrl.searchParams;
  console.log("OAUTHCALLBACK", searchParams);
  // let query = requestUrl.searchParams;

  /*
   url.parse(request.nextUrl.toString(), true)
    .query as ParsedUrlQuery as {
    code: string;
    scope: string;
  };
  */

  /**
   * To use OAuth2 authentication, we need access to a CLIENT_ID, CLIENT_SECRET, AND REDIRECT_URI
   * from the client_secret.json file. To get these credentials for your application, visit
   * https://console.cloud.google.com/apis/credentials.
   */
  const oauth2Client = new google.auth.OAuth2(
    GOOGLE_CLIENT_ID,
    GOOGLE_CLIENT_SECRET,
    GOOGLE_REDIRECT_URL
  );

  const { tokens } = await oauth2Client.getToken(
    searchParams.get("code") ?? ""
  );
  console.log("TOKENS", tokens);

  oauth2Client.setCredentials(tokens);

  const calendar = google.calendar("v3");
  const calenders = calendar.calendarList.list();
  const events = calendar.events.list();
  console.log("calenders", calenders);
  console.log("events", events);
  const cookieString = Object.entries(tokens)
    .map(([key, value]) => `${key}=${value}`)
    .join("&");
  console.log("cookieString", cookieString);

  return new NextResponse(JSON.stringify({ calenders, events }), {
    status: 200,
    headers: { "Set-Cookie": cookieString },
  });
}

// export const dynamic = "force-dynamic"; // defaults to auto
