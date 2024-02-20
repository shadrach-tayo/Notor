import {
  Schema$CalendarListEntry,
  Schema$Event,
  // Schema$Events,
} from "@/services/api/googleCalendar";
import { PayloadAction, createSlice } from "@reduxjs/toolkit";

type CalendarsState = {
  calendars: Schema$CalendarListEntry[];
  events: Schema$Event[];
  eventsByCalendar: { [key: string]: Schema$Event[] };
};

// TODO: store events by calendar Id
const initialState: CalendarsState = {
  calendars: [],
  events: [],
  eventsByCalendar: {},
};

const calendars = createSlice({
  name: "calendarsList",
  initialState,
  reducers: {
    setCalendars: (
      state,
      { payload }: PayloadAction<Schema$CalendarListEntry[]>
    ) => {
      state.calendars = payload;
    },
    setEvents: (state, { payload }: PayloadAction<Schema$Event[]>) => {
      state.events = payload;
    },
  },
});

export default calendars.reducer;
export const { setCalendars, setEvents } = calendars.actions;
