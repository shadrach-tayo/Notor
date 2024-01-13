"use client";
// import Image from "next/image";
import { Button } from "@/components/ui/button";
import { useEffect, useRef } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/tauri";

type GoogleAuthToken = {
  access_token: string;
  token_type: string;
  expires_in: number;
  refresh_token: string;
  scope: string;
  expires_at: number;
};

export default function Home() {
  const loadedEventRef = useRef(false);

  useEffect(() => {}, []);

  useEffect(() => {
    let unlisten = () => {};

    const invokeLoadCommand = async () => {
      let credentials = await invoke<GoogleAuthToken>("app_loaded", {});
      console.log(credentials);
    };

    const registerListener = async () => {
      unlisten = await listen<GoogleAuthToken>(
        "GOOGLE_AUTH_CREDENTIALS",
        (event) => {
          console.log(
            `Got error in window ${event.windowLabel}, payload: ${event.payload}`
          );
        }
      );
    };

    if (window && loadedEventRef.current === false) {
      console.log("EMIT LOADED EVENT");
      invokeLoadCommand();
      loadedEventRef.current = true;
    }

    registerListener();

    return () => {};
  }, []);
  return (
    <main className="flex min-h-screen flex-col items-center justify-between rounded-md p-24 backdrop-blur-md">
      <h1>Upcoming Events View</h1>
    </main>
  );
}
