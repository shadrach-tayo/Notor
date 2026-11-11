"use client";

import { Button } from "@/components/ui/button";
import { useEffect, useState } from "react";

type Phase = {
  title: string;
  detail?: string;
  tryAgain?: boolean;
  cancel?: boolean;
  openPage?: boolean;
};

function phaseCopy(code: string): Phase {
  switch (code) {
    case "waiting":
      return {
        title: "Waiting for Google…",
        detail: "Finish in the browser window that just opened.",
        cancel: true,
      };
    case "access_denied":
      return {
        title: "Sign-in cancelled",
        detail: "Notor did not receive permission.",
        tryAgain: true,
      };
    case "blocked":
      return {
        title: "Google blocked this sign-in",
        detail: "Ask your administrator to allow Notor.",
        tryAgain: true,
      };
    case "timed_out":
      return {
        title: "Sign-in timed out",
        detail: "Start again to get a new sign-in link.",
        tryAgain: true,
      };
    case "cancelled_in_app":
      return {
        title: "This sign-in was cancelled in Notor",
        tryAgain: true,
      };
    case "offline":
      return {
        title: "Couldn't connect",
        detail: "Check the connection and try again.",
        tryAgain: true,
      };
    case "upstream":
    case "no_email":
    case "unsafe_url":
      return {
        title: "The sign-in service had a problem",
        tryAgain: true,
      };
    case "needs_calendar":
      return {
        title: "Notor needs to see your calendars",
        detail: "Allow calendar access, then try again.",
        tryAgain: true,
      };
    case "open_failed":
      return {
        title: "The browser did not open",
        detail: "Open sign-in page",
        openPage: true,
      };
    case "too_many":
      return {
        title: "Too many sign-ins are waiting",
        detail: "Finish one of them, or wait a few minutes.",
        tryAgain: true,
      };
    case "done":
      return { title: "Signed in" };
    default:
      return { title: "Continue with Google" };
  }
}

export default function SignInPage() {
  const [code, setCode] = useState("idle");
  const [url, setUrl] = useState("");

  useEffect(() => {
    const preview = new URLSearchParams(window.location.search).get("preview");
    if (preview) setCode(preview);
  }, []);

  useEffect(() => {
    let stop = () => {};
    let cancelled = false;
    (async () => {
      try {
        // Tauri 1 has no event allowlist. listen and unlisten are available here.
        const { listen } = await import("@tauri-apps/api/event");
        const unlisten = await listen<{ ok?: boolean; error?: string }>(
          "google-signin",
          (event) => {
            if (event.payload?.ok) setCode("done");
            else if (event.payload?.error) setCode(event.payload.error);
          },
        );
        if (cancelled) unlisten();
        else stop = unlisten;
      } catch {
        // The page is open outside the desktop app.
      }
    })();
    return () => {
      cancelled = true;
      stop();
    };
  }, []);

  useEffect(() => {
    if (code !== "waiting") return;
    const timer = window.setTimeout(() => setCode("timed_out"), 10 * 60 * 1000);
    return () => window.clearTimeout(timer);
  }, [code]);

  async function connect() {
    setCode("waiting");
    try {
      const { invoke } = await import("@tauri-apps/api/tauri");
      const result = await invoke<{ phase?: string; url?: string }>("start_google_login");
      if (result?.phase === "open_failed") {
        setUrl(result.url ?? "");
        setCode("open_failed");
        return;
      }
      setCode("waiting");
    } catch (err) {
      const message = typeof err === "string" ? err : "offline";
      setCode(message || "offline");
    }
  }

  async function cancel() {
    try {
      const { invoke } = await import("@tauri-apps/api/tauri");
      await invoke("cancel_google_login");
    } catch {
      // The window still shows that sign-in was cancelled here.
    }
    setCode("cancelled_in_app");
  }

  async function reopen() {
    try {
      const { invoke } = await import("@tauri-apps/api/tauri");
      await invoke("open_sign_in_page");
    } catch {
      if (url) window.open(url, "_blank", "noopener,noreferrer");
    }
  }

  const phase = phaseCopy(code);

  return (
    <main className="bg-background flex h-full min-h-screen flex-col items-center justify-center gap-4 rounded-md p-24 text-center">
      <h1 className="text-xl font-medium">{phase.title}</h1>
      {phase.detail ? <p className="max-w-md text-sm opacity-80">{phase.detail}</p> : null}
      {code === "idle" ? (
        <Button type="button" onClick={connect}>
          Continue with Google
        </Button>
      ) : null}
      {phase.cancel ? (
        <Button type="button" variant="ghost" onClick={cancel}>
          Cancel
        </Button>
      ) : null}
      {phase.tryAgain ? (
        <Button type="button" onClick={connect}>
          Try again
        </Button>
      ) : null}
      {phase.openPage ? (
        <Button type="button" onClick={reopen}>
          Open sign-in page
        </Button>
      ) : null}
    </main>
  );
}
