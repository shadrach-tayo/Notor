"use client";
import { Button } from "@/components/ui/button";
// import { useParams, usePathname } from "next/navigation";
import { open } from "@tauri-apps/api/shell";

export default function Home() {
  // const params = useParams();
  // const pathname = usePathname();
  // // console.log("PARAMS", { params, pathname });

  const connect = async () => {
    const response = await fetch(`${process.env.NEXT_PUBLIC_API_SERVER}/login/google`).then(
      (res) => res.json() as unknown as { url: string }
    );
    console.log("response", response.url);
    if (response.url) open(response.url);
  };

  return (
    <main
      data-tauri-drag-region
      className="bg-background flex h-full min-h-screen flex-col items-center justify-center rounded-md p-24 backdrop-blur-md"
    >
      <Button
        variant="ghost"
        className="bg-primary-foreground hover:bg-secondary"
        onClick={connect}
      >
        Continue with Google
      </Button>
    </main>
  );
}
