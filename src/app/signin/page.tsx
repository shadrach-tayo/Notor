"use client";
import { Button } from "@/components/ui/button";
// import { useParams, usePathname } from "next/navigation";
import { open } from "@tauri-apps/api/shell";
import {API_SERVER} from "@/lib/config";

export default function Home() {

  const connect = async () => {
    try {
      const response = await fetch(`${API_SERVER}/login/google`).then(
        (res) => res.json() as unknown as { url: string }
      );
      console.log("response", response.url);
      if (response.url) await open(response.url);
    } catch (err) {
      console.error("Google sign-in failed", err);
    }
  };

  return (
    <main
      data-tauri-drag-region
      className="bg-background flex h-full min-h-screen flex-col items-center justify-center rounded-md p-24 backdrop-blur-md"
    >
      <Button
        type="button"
        data-tauri-drag-region="false"
        variant="ghost"
        className="bg-primary-foreground hover:bg-secondary"
        onClick={connect}
      >
        Continue with Google
      </Button>
    </main>
  );
}
