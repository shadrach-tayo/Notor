import React from "react";
import AppProviders from "@/AppProviders";

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return <AppProviders>{children}</AppProviders>;
}
