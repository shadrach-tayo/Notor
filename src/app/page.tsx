"use client";
// import Image from "next/image";
import { Button } from "@/components/ui/button";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/tauri";
import { GoogleAuthToken, setToken } from "@/slices/authSlice";
import { useSetter } from "@/store/accessors";
import { useUserInfoQuery } from "@/services/api/auth";
import { useAuthToken, useUser } from "@/slices/hooks";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Schema$Event,
  useCalendarListQuery,
  useEventsQuery,
  useLazyEventsQuery,
} from "@/services/api/googleCalendar";
import { setEvents } from "@/slices/calendars";

export default function Home() {
  const authToken = useAuthToken();
  const dispatch = useSetter();

  useUserInfoQuery(authToken?.access_token!, {
    skip: !authToken?.access_token,
  });

  const { data: calendars } = useCalendarListQuery(authToken?.access_token!, {
    skip: !authToken?.access_token,
    refetchOnFocus: true,
    refetchOnMountOrArgChange: true,
    refetchOnReconnect: true,
  });

  const [queryEvent, { isLoading, isFetching, isError }] = useLazyEventsQuery();

  const aggregateCalendarEvents = useCallback(async () => {
    const eventsPromise =
      calendars && calendars.length > 0
        ? calendars.map((cal) =>
            queryEvent({
              // accessToken: authToken?.access_token!,
              calendarId: cal.id!,
            })
          )
        : [];
    const results = await Promise.all(eventsPromise);

    const events = results
      .filter((evt) => evt.isSuccess)
      .map((result) => result.data)
      .flat();

    if (events && events.length > 0) {
      dispatch(setEvents(events as Schema$Event[]));
    }
  }, [calendars, dispatch, queryEvent]);

  useEffect(() => {
    if (calendars) {
      aggregateCalendarEvents();
    }
  }, [aggregateCalendarEvents, calendars]);

  const loadedEventRef = useRef(false);
  const user = useUser();

  const logout = async () => {
    await invoke("logout");
  };

  useEffect(() => {
    let unlisten = () => {};

    const invokeLoadCommand = async () => {
      let credentials = await invoke<GoogleAuthToken>("app_loaded");
      console.log({ credentials });
      dispatch(setToken({ provider: "google", token: credentials }));
    };

    const registerListener = async () => {
      unlisten = await listen<GoogleAuthToken>(
        "GOOGLE_AUTH_CREDENTIALS",
        async (event) => {
          console.log("Login event", event);
          dispatch(setToken({ provider: "google", token: event.payload }));
        }
      );
    };

    if (window && loadedEventRef.current === false) {
      console.log("EMIT LOADED EVENT");
      invokeLoadCommand();
      loadedEventRef.current = true;
    }

    registerListener();

    return () => {};
  }, [dispatch]);

  return (
    <main className="flex flex-col items-start rounded-md p-1 px-2 backdrop-blur-md">
      <div className="w-full flex justify-between items-center">
        <h1 className="self-start text-sm">Hey, {user.given_name}</h1>
        <div className="flex place-self-end align-bottom self-end">
          <DropdownMenu>
            <DropdownMenuTrigger>
              <Avatar className="w-6 h-6 shadow-xl drop-shadow-2xl">
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
