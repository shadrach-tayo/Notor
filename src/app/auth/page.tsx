"use client";

import { Button } from "@/components/ui/button";
import { RPC_SERVER } from "@/lib/config";
import { useEffect, useRef, useState } from "react";

type View = {
  title: string;
  detail?: string;
  retry?: boolean;
};

function viewFor(code: string): View {
  switch (code) {
    case "access_denied":
      return {
        title: "Sign-in cancelled",
        detail: "Go back to Notor and try again.",
      };
    case "blocked":
      return {
        title: "Google blocked this sign-in",
        detail: "Ask your administrator to allow Notor.",
      };
    case "timed_out":
      return {
        title: "Sign-in timed out",
        detail: "Go back to Notor and start again.",
      };
    case "cancelled_in_app":
      return { title: "This sign-in was cancelled in Notor" };
    case "offline":
      return {
        title: "Couldn't reach Notor",
        detail: "Open Notor on this computer, then try again.",
        retry: true,
      };
    case "already_finished":
      return { title: "This sign-in is already finished" };
    case "unknown_state":
      return { title: "This sign-in didn’t start on this computer" };
    case "nothing_to_finish":
      return { title: "There is nothing to finish" };
    case "needs_calendar":
      return {
        title: "Notor needs to see your calendars",
        detail: "Go back to Notor and allow calendar access.",
      };
    default:
      return {
        title: "The sign-in service had a problem",
        detail: "Go back to Notor and try again.",
      };
  }
}

type Payload = { state: string; code?: string; error?: string };

export default function AuthPage() {
  const [view, setView] = useState<View>({ title: "Finishing sign-in…" });
  const payloadRef = useRef<Payload | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const preview = params.get("preview");
    if (preview && attempt === 0 && payloadRef.current === null) {
      window.history.replaceState({}, "", window.location.pathname);
      setView(viewFor(preview));
      return;
    }

    if (payloadRef.current === null) {
      const state = params.get("state") ?? "";
      const code = params.get("code") ?? "";
      const error = params.get("error") ?? "";
      window.history.replaceState({}, "", window.location.pathname);
      if (!state && !code && !error) {
        setView(viewFor("nothing_to_finish"));
        return;
      }
      payloadRef.current = error ? { state, error } : { state, code };
    }

    const payload = payloadRef.current;
    if (!payload) return;
    let cancelled = false;
    (async () => {
      try {
        const response = await fetch(`${RPC_SERVER}/api/google_auth`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        });
        const body = (await response.json().catch(() => ({}))) as {
          ok?: boolean;
          error?: string;
        };
        if (cancelled) return;
        if (response.ok && body.ok) {
          setView({
            title: "Signed in",
            detail: "You can close this page and return to Notor.",
          });
          return;
        }
        const reported = typeof body.error === "string" ? body.error : "";
        if (reported) {
          setView(viewFor(reported));
          return;
        }
        if (payload.error === "access_denied") {
          setView(viewFor("access_denied"));
          return;
        }
        if (payload.error) {
          setView(viewFor("blocked"));
          return;
        }
        setView(viewFor("upstream"));
      } catch {
        if (cancelled) return;
        if (payload.error === "access_denied") setView(viewFor("access_denied"));
        else if (payload.error) setView(viewFor("blocked"));
        else setView(viewFor("offline"));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  return (
    <main className="bg-background flex h-full min-h-screen flex-col items-center justify-center gap-4 rounded-md p-24 text-center">
      <h1 className="text-xl font-medium">{view.title}</h1>
      {view.detail ? <p className="max-w-md text-sm opacity-80">{view.detail}</p> : null}
      {view.retry ? (
        <Button type="button" onClick={() => setAttempt((value) => value + 1)}>
          Try again
        </Button>
      ) : null}
    </main>
  );
}
