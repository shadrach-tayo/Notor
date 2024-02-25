import React from "react";
import { Providers } from "@/AppProviders";

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return <Providers>{children}</Providers>;
}
