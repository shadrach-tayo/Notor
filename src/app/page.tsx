"use client";
import AppProviders from "@/AppProviders";
import CustomTrayApp from "@/components/ui/CustomTray";

export default function Home() {
  return (
    <AppProviders>
      <CustomTrayApp />
    </AppProviders>
  );
}
