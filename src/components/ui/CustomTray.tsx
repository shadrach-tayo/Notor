"use client";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
// import {
//   DropdownMenu,
//   DropdownMenuContent,
//   DropdownMenuItem,
//   DropdownMenuLabel,
//   DropdownMenuSeparator,
//   DropdownMenuTrigger,
// } from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
// import { AvatarImage, AvatarFallback, Avatar } from "@/components/ui/avatar"
import {
  AccordionTrigger,
  AccordionContent,
  AccordionItem,
  Accordion,
} from "@/components/ui/accordion";
import { useUserInfoQuery } from "@/services/api/auth";
import { useAuthToken, useEventsGroups, useUser } from "@/slices/hooks";
import { invoke } from "@tauri-apps/api/tauri";
import { CalendarIcon, EyeIcon, TrashIcon } from "@/components/icons/icons";
import { EyeOffIcon } from "lucide-react";
import { GoogleAuthToken, setToken } from "@/slices/authSlice";
import { useEffect, useState } from "react";
import clsx from "clsx";

export default function CustomTrayApp() {
  // console.log(`API SERVER ${process.env.NEXT_PUBLIC_API_SERVER}`);
  // const authToken = useAuthToken();
  const [accounts, setAccounts] = useState<GoogleAuthToken[]>([]);

  // const user = useUser();

  const logout = async () => {
    await invoke("logout");
  };

  const invoke_list_accounts = async () => {
    let accounts = await invoke<GoogleAuthToken[]>("list_accounts");
    console.log("Calendar accounts", accounts);
    setAccounts(accounts);
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
              <AccordionContent className="space-y-4 pt-4">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <CalendarIcon className="w-5 h-5 text-gray-500 dark:text-gray-400" />
                    <p className="text-sm font-medium">Personal Calendar</p>
                  </div>
                  <Button className="rounded-full" size="icon" variant="ghost">
                    <EyeIcon className="w-5 h-5 text-gray-500 dark:text-gray-400" />
                    <span className="sr-only">Toggle visibility</span>
                  </Button>
                </div>
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <CalendarIcon className="w-5 h-5 text-gray-500 dark:text-gray-400" />
                    <p className="text-sm font-medium">Work Calendar</p>
                  </div>
                  <Button className="rounded-full" size="icon" variant="ghost">
                    <EyeOffIcon className="w-5 h-5 text-gray-500 dark:text-gray-400" />
                    <span className="sr-only">Toggle visibility</span>
                  </Button>
                </div>
                {account.user && (
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
                )}
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      </div>
    </main>
  );
}
