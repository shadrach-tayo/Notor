import {
  GOOGLE_CLIENT_ID,
  GOOGLE_CLIENT_SECRET,
  GOOGLE_REDIRECT_URL,
} from "@/lib/config";
// import { cookies } from "next/headers";
import { google } from "googleapis";
// export const revalidate = "force-cache";
export async function GET(request: Request) {
  // const cookieStore = cookies();
  // const token = cookieStore.get("token");

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

  // Access scopes for read-only Drive activity.
  const scopes = [
    "https://www.googleapis.com/auth/calendar.readonly",
    // "https://www.googleapis.com/auth/drive.metadata.readonly",
  ];

  // Generate a url that asks permissions for the Drive activity scope
  const authorizationUrl = oauth2Client.generateAuthUrl({
    // 'online' (default) or 'offline' (gets refresh_token)
    access_type: "offline",
    /** Pass in the scopes array defined above.
     * Alternatively, if only one scope is needed, you can pass a scope URL as a string */
    scope: scopes,
    // Enable incremental authorization. Recommended as a best practice.
    include_granted_scopes: true,
  });

  return new Response(JSON.stringify({ url: authorizationUrl }), {
    status: 200,
    // headers: { "Set-Cookie": `token=${token?.value}` },
  });
}
