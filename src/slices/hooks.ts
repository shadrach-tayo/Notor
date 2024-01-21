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

export const useEventDetails = () => {
  const events = useEvents();
  const now = new Date().getTime();
  const activeEvents = events.filter((event) => {
    // const startTime = new Date(event.start?.dateTime);
    const isActive = event.start?.dateTime;
  });
  // active Events -> events happening now
  // upcoming events -> events happening today + tomorrow
};
