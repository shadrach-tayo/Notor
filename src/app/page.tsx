"use client";
// import Image from "next/image";
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

export default function Home() {
  const authToken = useAuthToken();
  // const dispatch = useSetter();
  useEventsGroups();

  useUserInfoQuery(authToken?.access_token!, {
    skip: !authToken?.access_token,
  });

  // const { data: calendars } = useCalendarListQuery(authToken?.access_token!, {
  //   skip: !authToken?.access_token,
  //   refetchOnFocus: true,
  //   refetchOnMountOrArgChange: true,
  //   refetchOnReconnect: true,
  // });

  // const [queryEvent, { isLoading, isFetching, isError }] = useLazyEventsQuery();

  // const aggregateCalendarEvents = useCallback(async () => {
  //   const eventsPromise =
  //     calendars && calendars.length > 0
  //       ? calendars.map((cal) =>
  //           queryEvent({
  //             calendarId: cal.id!,
  //           })
  //         )
  //       : [];
  //   const results = await Promise.all(eventsPromise);

  //   const events = results
  //     .filter((evt) => evt.isSuccess)
  //     .map((result) => result.data)
  //     .flat();

  //   if (events && events.length > 0) {
  //     dispatch(setEvents(events as Schema$Event[]));
  //   }
  // }, [calendars, dispatch, queryEvent]);

  // useEffect(() => {
  //   if (calendars) {
  //     aggregateCalendarEvents();
  //   }
  // }, [aggregateCalendarEvents, calendars]);

  // const loadedEventRef = useRef(false);
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
