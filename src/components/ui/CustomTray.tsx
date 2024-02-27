"use client";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useUserInfoQuery } from "@/services/api/auth";
import { useAuthToken, useEventsGroups, useUser } from "@/slices/hooks";
import { invoke } from "@tauri-apps/api/tauri";

export default function CustomTrayApp() {
  console.log(`API SERVER ${process.env.NEXT_PUBLIC_API_SERVER}`);
  const authToken = useAuthToken();
  useEventsGroups();

  useUserInfoQuery(authToken?.access_token!, {
    skip: !authToken?.access_token,
  });

  const user = useUser();

  const logout = async () => {
    await invoke("logout");
  };

  return (
    <main className="flex flex-col items-start rounded-md p-1 px-2 backdrop-blur-md">
      <div className="flex w-full items-center justify-between">
        <h1 className="self-start text-sm">Hey, {user.given_name}</h1>
        <div className="flex place-self-end self-end align-bottom">
          <DropdownMenu>
            <DropdownMenuTrigger>
              <Avatar className="h-6 w-6 shadow-xl drop-shadow-2xl">
                <AvatarImage src={user.picture} />
                <AvatarFallback>SO</AvatarFallback>
              </Avatar>
            </DropdownMenuTrigger>
            <DropdownMenuContent>
              <DropdownMenuLabel>My Account</DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuItem>Settings</DropdownMenuItem>
              <DropdownMenuItem onClick={logout}>Logout</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <div className="my-1"></div>
      <h1>Upcoming Events</h1>
    </main>
  );
}
