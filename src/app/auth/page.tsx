"use client";
import { Button } from "@/components/ui/button";
import { useEffect, useState } from "react";

export default function Home() {
  const [isLoading, setIsLoading] = useState(false);

  const completeSignin = async () => {
    const response = await fetch(
      `http://localhost:4876/oauth2callback/google${location.search}`,
      {
        headers: {
          Authorization: `Bearer ********************************`,
        },
      }
    );

    console.log("response", response.status, response.statusText);
    if (response.ok) {
      // retrieve json_token {} and send to internal tauri server
      const jsonToken = await response.json();
      console.log("response", jsonToken);
      const postResponse = await fetch(
        "http://localhost:4875/api/google_auth",
        { method: "POST", body: JSON.stringify(jsonToken) }
      );
      console.log("response", postResponse.status, postResponse.statusText);
    }
    // if (response.url) window.open(response.url, "_blank");
  };

  useEffect(() => {
    if (isLoading) return;
    setIsLoading(true);
    completeSignin();
  }, [isLoading]);

  return (
    <main
      data-tauri-drag-region
      className="bg-primary flex h-full min-h-screen flex-col items-center justify-center rounded-md p-24 backdrop-blur-md"
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
