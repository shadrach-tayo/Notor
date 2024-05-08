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
import {GoogleAuthToken, setToken} from "@/slices/authSlice";
import {setEvents} from "@/slices/calendars";
import {setAlert} from "@/slices/alert";
import {useAuthToken, useEventsGroups, useUser} from "@/slices/hooks";
import {useSetter} from "@/store/accessors";
import {listen} from "@tauri-apps/api/event";
import {invoke} from "@tauri-apps/api/tauri";
// import {scheduler} from "@/lib/Scheduler";
// import {EventStartEvent} from "@/lib/customEvents";

import {
    isPermissionGranted,
    requestPermission,
    sendNotification,
} from "@tauri-apps/api/notification";

const standUpEvent = {
    "kind": "calendar#event",
    "etag": "\"3428187424877000\"",
    "id": "r6oc5auhrtbccrnk4nfg1c1345_20240426T113000Z",
    "status": "confirmed",
    "htmlLink": "https://www.google.com/calendar/event?eid=cjZvYzVhdWhydGJjY3JuazRuZmcxYzEzNDVfMjAyNDA0MjZUMTEzMDAwWiBzaGFkcmFjaEBkZXNjaS5jb20",
    "created": "2022-11-25T00:36:42.000Z",
    "updated": "2024-04-26T01:08:33.710Z",
    "summary": "Europe Dev Standup",
    "description": "- Discuss what we are working on and if you have any blockers\n\n- If you can't make it just send your status to #dev-standup",
    "location": "https://us02web.zoom.us/j/81727881719",
    "creator": {
        "email": "sina@desci.com"
    },
    "organizer": {
        "email": "sina@desci.com"
    },
    "start": {
        "dateTime": "2024-04-27T13:30:00+02:00",
        "timeZone": "America/New_York"
    },
    "end": {
        "dateTime": "2024-04-27T13:45:00+02:00",
        "timeZone": "America/New_York"
    },
    "recurringEventId": "r6oc5auhrtbccrnk4nfg1c1345_R20240422T113000",
    "originalStartTime": {
        "dateTime": "2024-04-27T13:30:00+02:00",
        "timeZone": "America/New_York"
    },
    "iCalUID": "r6oc5auhrtbccrnk4nfg1c1345_R20240422T113000@google.com",
    "sequence": 2,
    "attendees": [
        {
            "email": "sina@desci.com",
            "organizer": true,
            "responseStatus": "accepted"
        },
        {
            "email": "adam@desci.com",
            "responseStatus": "needsAction"
        },
        {
            "email": "shadrach@desci.com",
            "self": true,
            "responseStatus": "accepted"
        },
        {
            "email": "edvard@desci.com",
            "responseStatus": "declined"
        },
        {
            "email": "andre@desci.com",
            "responseStatus": "needsAction"
        },
        {
            "email": "aseer@desci.com",
            "responseStatus": "accepted"
        },
        {
            "email": "carla@desci.com",
            "responseStatus": "accepted"
        }
    ],
    "reminders": {
        "useDefault": false,
        "overrides": [
            {
                "method": "popup",
                "minutes": 0
            },
            {
                "method": "popup",
                "minutes": 10
            }
        ]
    },
    "attachments": [
        {
            "fileUrl": "https://docs.google.com/document/d/1KaEcDwHUlpnYYPMkZlOYQRYzv1zseQf8LOXYozLmSeE/edit",
            "title": "Notes - Europe Dev Standup",
            "mimeType": "application/vnd.google-apps.document",
            "iconLink": "https://drive-thirdparty.googleusercontent.com/16/type/application/vnd.google-apps.document",
            "fileId": "1KaEcDwHUlpnYYPMkZlOYQRYzv1zseQf8LOXYozLmSeE"
        }
    ],
    "eventType": "default"
};
export default function EventsProvider(props: PropsWithChildren<unknown>) {
    const authToken = useAuthToken();
    const user = useUser();
    const dispatch = useSetter();
    const eventGroups = useEventsGroups();
    const [permissionGranted, setPermissionGranted] = useState(false);
    console.log("EVENT GROUPS", eventGroups);

    const {data: calendars} = useCalendarListQuery(authToken?.access_token!, {
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

        let events = results
            .filter((evt) => evt.isSuccess)
            .map((result) => result.data)
            .flat()
            .filter(evt =>
                evt?.attendees?.some(attendee => attendee.email === user.email)
                || (evt?.creator?.email === user.email && evt?.creator?.self === true)
                || false
            )


        if (events && events.length > 0) {
            console.log("aggregateEvents", events);
            dispatch(
                setEvents(events as Schema$Event[])
            )
        }
    }, [user, calendars, dispatch, queryEvent]);

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
        let unlisten = () => {
        };

        const invokeLoadCommand = async () => {
            let credentials = await invoke<GoogleAuthToken>("app_loaded");
            console.log({credentials});
            dispatch(setToken({provider: "google", token: credentials}));
        };

        const registerListener = async () => {
            unlisten = await listen<GoogleAuthToken>(
                "GOOGLE_AUTH_CREDENTIALS",
                async (event) => {
                    console.log("GOOGLE_AUTH_CREDENTIALS::EVENT", event.payload);
                    // const token = typeof event.payload === "string" ? JSON.parse(event.payload) : event.payload
                    dispatch(setToken({provider: "google", token: event.payload as GoogleAuthToken}));
                }
            );
            await listen<Schema$Event>(
                "alert",
                async (event) => {
                    console.log("EVENT", event.payload);
                    dispatch(setAlert(event.payload));
                }
            );
        };

        if (window && !loadedEventRef.current) {
            console.log("EMIT LOADED EVENT");
            invokeLoadCommand();
            loadedEventRef.current = true;
        }

        registerListener();

        return () => {
        };
    }, [dispatch]);

    const triggerEventAlert = async (title: string) => {
        await invoke("show_alert", {title});
        console.log("SHOW ALERT:", title);
    };

    // const eventStartCallback = useCallback(
    //     (payload: EventStartEvent) => {
    //         console.log("EVENT start", payload);
    //         sendPushNotification(payload.detail.event);
    //         dispatch(setAlert(payload.detail.event));
    //         triggerEventAlert(payload.detail.event.id ?? '');
    //     },
    //     [dispatch]
    // );

    useEffect(() => {
        const updateTrayApp = async () => {
            let res = await invoke("build_events", {events: eventGroups});
            console.log("UPDATED TRAY APP", res);
        };

        updateTrayApp();
    }, [eventGroups]);

    useEffect(() => {
        if (eventGroups.upcoming.length === 0) return;

        if (eventGroups.upcoming.length > 0) {
            invoke("schedule_events", {events: eventGroups.upcoming})
            console.log("scheduled", eventGroups.upcoming.length);
        }

        // window.addEventListener("eventStart", eventStartCallback);

        // return () => window.removeEventListener("eventStart", eventStartCallback);
    }, [eventGroups]);

    return <>{props.children}</>;
}
