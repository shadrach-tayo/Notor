"use client";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
// import { AvatarImage, AvatarFallback, Avatar } from "@/components/ui/avatar"
import {
  AccordionTrigger,
  AccordionContent,
  AccordionItem,
  Accordion,
} from "@/components/ui/accordion";
import { invoke } from "@tauri-apps/api/tauri";
import { TrashIcon } from "@/components/icons/icons";
import { GoogleAuthToken } from "@/slices/authSlice";
import { useEffect, useState } from "react";
import clsx from "clsx";
import Calendars from "./Calendars";
import { Preferences } from "@/types/account";

export default function CustomTrayApp() {
  const [accounts, setAccounts] = useState<GoogleAuthToken[]>([]);
  const [preferences, setPreferences] = useState<Preferences>();

  const invoke_list_accounts = async () => {
    let accounts = await invoke<GoogleAuthToken[]>("list_accounts");
    console.log("Calendar accounts", accounts);
    setAccounts(accounts);
  };

  const invoke_get_preferences = async () => {
    let preferences = await invoke<Preferences>("get_preferences");
    console.log("Calendar preferences", preferences);
    setPreferences(preferences);
  };

  const invoke_hide_calendar = async (email: string, calendarId: string) => {
    await invoke<Preferences>("hide_calendar", { email, calendarId })
      .then((_) => {
        console.log("Hide Calendar");
        invoke_get_preferences();
      })
      .catch((err) => {
        console.log("Error: Hide Calendar", err);
      });
  };

  const invoke_show_calendar = async (email: string, calendarId: string) => {
    await invoke<Preferences>("show_calendar", { email, calendarId })
      .then((_) => {
        console.log("Show Calendar");
        invoke_get_preferences();
      })
      .catch((err) => {
        console.log("Error: Show Calendar", err);
      });
  };

  const removeAccount = async (email: string) => {
    await invoke<GoogleAuthToken[]>("remove_account", { email }).then((_) => {
      console.log("Account removed: ", email);
      invoke_list_accounts();
    });
  };

  const disableAccount = async (email: string) => {
    await invoke<GoogleAuthToken[]>("disable_account", { email })
      .then((_) => {
        console.log("Account disabled: ", email);
        invoke_list_accounts();
      })
      .catch((err) => console.log("Account not disabled:", err));
  };

  const enableAccount = async (email: string) => {
    await invoke<GoogleAuthToken[]>("enable_account", { email })
      .then((_) => {
        console.log("Account enabled: ", email);
        invoke_list_accounts();
      })
      .catch((err) => console.log("Account not enabled:", err));
  };

  useEffect(() => {
    invoke_list_accounts();
    invoke_get_preferences();
  }, []);

  return (
    <main className="flex flex-col items-start rounded-md p-1 px-2 backdrop-blur-md">
      <h1 className="text-xl mb-4">Preferences</h1>
      <div className="grid gap-6 w-full">
        <Accordion collapsible type="single">
          {accounts.map((account, idx) => (
            <AccordionItem
              key={account.user?.email}
              value={account.user?.email ?? `account-${idx}`}
            >
              <AccordionTrigger className="flex items-center gap-4 hover:no-underline">
                <Avatar className="w-5 h-5">
                  <AvatarImage alt="Avatar" src={account.user?.picture} />
                  <AvatarFallback>
                    {account.user?.given_name.substring(0, 1)}
                    {account.user?.family_name.substring(0, 1)}
                  </AvatarFallback>
                </Avatar>
                <div className="flex-1">
                  <p
                    className={clsx(
                      "text-sm font-medium truncate line-clamp-1",
                      account.disabled && "text-gray-200",
                    )}
                  >
                    {account.user?.email}
                  </p>
                </div>
              </AccordionTrigger>
              <AccordionContent className="space-y-4 py-1">
                {account.user && (
                  <>
                    <Calendars
                      accountPreferences={
                        preferences?.accounts_preferences?.[
                          account.user?.email ?? ""
                        ]
                      }
                      accessToken={account.access_token}
                      onToggleCalendar={(
                        calendar_id: string,
                        hide: boolean,
                      ) => {
                        if (!account.user?.email) return;

                        hide
                          ? invoke_hide_calendar(
                              account.user.email,
                              calendar_id,
                            )
                          : invoke_show_calendar(
                              account.user.email,
                              calendar_id,
                            );
                      }}
                    />
                    <div className="flex items-center justify-between gap-3">
                      <Button
                        className="rounded-md px-2 py-1.5 gap-2 bg-gray-600"
                        variant="ghost"
                        onClick={() =>
                          account.user?.email && account.disabled
                            ? enableAccount(account.user.email ?? "")
                            : disableAccount(account.user?.email ?? "")
                        }
                      >
                        <TrashIcon className="w-4 h-4" />
                        <span className="text-[12px]">
                          {account.disabled ? "Enable" : "Disable"} account
                        </span>
                        <span className="sr-only">
                          {account.disabled ? "Enable" : "Disable"} account
                        </span>
                      </Button>
                      <Button
                        className="rounded-md px-2 py-1.5 gap-2 bg-red-600 text-white hover:bg-red-500"
                        variant="ghost"
                        onClick={() =>
                          account.user?.email &&
                          removeAccount(account.user?.email)
                        }
                      >
                        <TrashIcon className="w-4 h-4" />
                        <span className="text-[12px]">Delete account</span>
                        <span className="sr-only">Delete account</span>
                      </Button>
                    </div>
                  </>
                )}
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      </div>
    </main>
  );
}
