import {
  PropsWithChildren,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  Schema$Event,
  useCalendarListQuery,
  useLazyEventsQuery,
} from "@/services/api/googleCalendar";
import { GoogleAuthToken, setToken } from "@/slices/authSlice";
import { setEvents } from "@/slices/calendars";
import { setAlert } from "@/slices/alert";
import { useAuthToken, useEventsGroups } from "@/slices/hooks";
import { useSetter } from "@/store/accessors";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/tauri";
import { scheduler } from "@/lib/Scheduler";
import { EventStartEvent } from "@/lib/customEvents";

import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/api/notification";

export default function EventsProvider(props: PropsWithChildren<unknown>) {
  const authToken = useAuthToken();
  const dispatch = useSetter();
  const eventGroups = useEventsGroups();
  const [permissionGranted, setPermissionGranted] = useState(false);
  console.log("EVENT GROUPS", eventGroups);

  const { data: calendars } = useCalendarListQuery(authToken?.access_token!, {
    skip: !authToken?.access_token,
    refetchOnFocus: true,
    refetchOnMountOrArgChange: true,
    refetchOnReconnect: true,
  });

  const [queryEvent] = useLazyEventsQuery();

  const sendPushNotification = async (event: Schema$Event) => {
    let permissionGranted = await isPermissionGranted();
    if (!permissionGranted) {
      const permission = await requestPermission();
      setPermissionGranted(permission === "granted");
    }
    if (permissionGranted) {
      // sendNotification(`${event.summary} starts now!`);
      sendNotification({
        title: `${event.summary}`,
        body: `${event.summary} starts now! at ${new Date(
          event.start?.date || event.start?.dateTime || ""
        ).toTimeString()}`,
        icon: "/public/static/icon.png", // "https://pub.desci.com/ipfs/bafkreicvyyyigqjpckswkhon2g2e73453pcjjj4slm7ycjojpr43fvrrlq", // "/static/icon.png"
        sound: "default",
      });
    }
  };

  const aggregateCalendarEvents = useCallback(async () => {
    let today = new Date();
    const timeMin = new Date();
    timeMin.setHours(0, 0, 0, 0);
    const timeMax = new Date(today.setDate(today.getDate() + 3));
    timeMax.setHours(23, 59, 0);
    const eventsPromise =
      calendars && calendars.length > 0
        ? calendars.map((cal) =>
            queryEvent({
              calendarId: cal.id!,
              timeMax: timeMax.toISOString(),
              timeMin: timeMin.toISOString(),
            })
          )
        : [];
    const results = await Promise.all(eventsPromise);

    const events = results
      .filter((evt) => evt.isSuccess)
      .map((result) => result.data)
      .flat();

    console.log("aggregateEvents", events.length);
    if (events && events.length > 0) {
      dispatch(setEvents(events as Schema$Event[]));
    }
  }, [calendars, dispatch, queryEvent]);

  useEffect(() => {
    let intervalId: NodeJS.Timeout;
    if (calendars) {
      // TODO: switch to google event watch api
      intervalId = setInterval(() => aggregateCalendarEvents(), 20000);
      aggregateCalendarEvents();
    }
    return () => {
      clearInterval(intervalId);
    };
  }, [aggregateCalendarEvents, calendars]);

  const loadedEventRef = useRef(false);

  useEffect(() => {
    let unlisten = () => {};

    const invokeLoadCommand = async () => {
      let credentials = await invoke<GoogleAuthToken>("app_loaded");
      console.log({ credentials });
      dispatch(setToken({ provider: "google", token: credentials }));
    };

    const registerListener = async () => {
      unlisten = await listen<string>(
        "GOOGLE_AUTH_CREDENTIALS",
        async (event) => {
          console.log("Login event", event);
          dispatch(
            setToken({ provider: "google", token: JSON.parse(event.payload) })
          );
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

  const triggerEventAlert = async () => {
    let res = await invoke("show_alert");
    console.log("SHOW ALERT", res);
  };

  const eventStartCallback = useCallback(
    (payload: EventStartEvent) => {
      console.log("EVENT start", payload);
      sendPushNotification(payload.detail.event);
      dispatch(setAlert(payload.detail.event));
      triggerEventAlert();
    },
    [dispatch]
  );

  useEffect(() => {
    const updateTrayApp = async () => {
      let res = await invoke("build_events", { events: eventGroups });
      console.log("UPDATED TRAY APP", res);
    };

    updateTrayApp();
  }, [eventGroups]);

  useEffect(() => {
    if (eventGroups.upcoming.length === 0) return;
    const scheduled = eventGroups.upcoming.map((evt) =>
      scheduler.scheduleEvent(evt)
    );
    console.log("scheduled", scheduled);

    window.addEventListener("eventStart", eventStartCallback);

    return () => window.removeEventListener("eventStart", eventStartCallback);
  }, [eventGroups, eventStartCallback]);

  return <>{props.children}</>;
}
