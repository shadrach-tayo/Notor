"use client";
// import Image from "next/image";
import { Button } from "@/components/ui/button";
import { useState, useRef } from "react";

export default function Home() {
  const CLIENT_ID = "";
  const API_KEY = "";
  // Discovery doc URL for APIs used by the quickstart
  const DISCOVERY_DOC =
    "https://www.googleapis.com/discovery/v1/apis/calendar/v3/rest";

  // Authorization scopes required by the API; multiple scopes can be
  // included, separated by spaces.
  const SCOPES = "https://www.googleapis.com/auth/calendar.readonly";

  const tokenClient = useRef();
  const [gapiInited, setGapInited] = useState(false);
  const [gisInited, setGisInited] = useState(false);

  return (
    <main
      data-tauri-drag-region
      className="bg-primary flex h-full min-h-screen flex-col items-center justify-center rounded-md p-24 backdrop-blur-md"
    >
      <Button
        variant="ghost"
        className="bg-primary-foreground hover:bg-secondary"
      >
        Continue with Google
      </Button>
    </main>
  );
}
