"use client";
import { Button } from "@/components/ui/button";
import { useEffect, useRef } from "react";
import {API_SERVER, RPC_SERVER} from "@/lib/config";
// import {useLazyUserInfoQuery} from "@/services/api/auth";

export default function Home() {
  const loadingRef = useRef(false);
  // const [queryUser] = useLazyUserInfoQuery();
  console.log("auth page");
  const completeSignin = async () => {
    try {
      const response = await fetch(
        `${API_SERVER}/oauth2callback/google${location.search}`
      );

      console.log("response", response.status, response.statusText);
      if (response.ok) {
        // retrieve json_token {} and send to internal tauri server
        const jsonToken = await response.json();
        console.log("response", jsonToken);
        let data = await fetch(`https://www.googleapis.com/oauth2/v2/userinfo?alt=json&access_token=${jsonToken.access_token}`, {
          headers: {
              "Content-Type": "application/json",
              Accept: "application/json",
            },
        })
        data = await data.json();
        let token = { ...jsonToken, user: data };
        console.log("User response", token);
        const postResponse = await fetch(
          `${RPC_SERVER}/api/google_auth`,
          { method: "POST", body: JSON.stringify(token) }
        );
        console.log("response", await postResponse.json());
      }
      loadingRef.current = false;
    } catch (err) {
      console.log("ERROR", err);
    }
  };

  useEffect(() => {
    if (loadingRef.current) return;
    loadingRef.current = true;
    completeSignin();
  }, []);

  return (
    <main
      data-tauri-drag-region
      className="bg-background flex h-full min-h-screen flex-col items-center justify-center rounded-md p-24 backdrop-blur-md"
    >
      <Button
        variant="ghost"
        className="bg-primary-foreground hover:bg-secondary"
      >
        Completing Google signin...
      </Button>
    </main>
  );
}
