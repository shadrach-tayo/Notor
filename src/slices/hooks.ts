import { Schema$Event } from "@/services/api/googleCalendar";
import { useGetter } from "@/store/accessors";

export const useUser = () => {
  return useGetter((state) => state.auth?.user);
};

export const useAuthToken = () => {
  return useGetter((state) => state.auth?.tokens["google"]);
};

export const useCalendars = () => {
  return useGetter((state) => state.calendars.calendars);
};

export const useEvents = () => {
  return useGetter((state) => state.calendars.events);
};

export const useAlert = () => {
  return useGetter((state) => state.alert.alert);
};

type EventGroupTags = "now" | "upcoming" | "tomorrow";
type EventGroups = Record<EventGroupTags, Schema$Event[]>;

function groupByStartTime(events: Schema$Event[]): EventGroups {
  const initial: EventGroups = {
    now: [],
    upcoming: [],
    tomorrow: [],
  };

  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(0, 0, 0, 0);
  const tomorrowEnd = new Date(tomorrow);
  tomorrowEnd.setHours(23, 59, 0, 0);

  return events.reduce((groups, event) => {
    const now = new Date().getTime();
    const start = new Date(
      event.start?.date || event.start?.dateTime || ""
    ).getTime();
    const end = new Date(
      event.end?.date || event.end?.dateTime || ""
    ).getTime();

    if (now > start && now < end) {
      groups.now.push(event);
    } else if (now < start && start < tomorrow.getTime()) {
      groups.upcoming.push(event);
    } else if (start > tomorrow.getTime() && start < tomorrowEnd.getTime()) {
      groups.tomorrow.push(event);
    }
    return groups;
  }, initial) as EventGroups;
}

export const useEventsGroups = () => {
  const events = useEvents();
  // const calendars = useCalendars();

  const groups = groupByStartTime(events);
  groups.now.sort(sortEvent);
  groups.upcoming.sort(sortEvent);
  groups.tomorrow.sort(sortEvent);
  // console.log("EVENT GROUPS", groups);
  return groups;
};

const sortEvent = (evt1: Schema$Event, evt2: Schema$Event) => {
  let time1 = new Date(
    evt1.start?.date || evt1.start?.dateTime! || Date.now()
  ).getTime();
  let time2 = new Date(
    evt2.start?.date || evt2.start?.dateTime! || Date.now()
  ).getTime();
  return time1 - time2;
};
