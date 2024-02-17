import { Schema$Event } from "@/services/api/googleCalendar";

export enum SchedulerEvents {
  EVENT_START = "eventStart",
  EVENT_REMINDER = "eventReminder",
}

export interface EventStartEvent extends Event {
  readonly detail: { event: Schema$Event };
}

export interface EventReminderEvent extends Event {
  readonly detail: { event: Schema$Event };
}

export interface CalendarEventMap {
  eventStart: EventStartEvent;
  eventReminder: EventReminderEvent;
}

export const dispatchEventNotification = (detail: EventStartEvent["detail"]) =>
  dispatchEvent(new CustomEvent(SchedulerEvents.EVENT_START, { detail }));

export const dispatchReminderNotification = (
  detail: EventReminderEvent["detail"]
) => dispatchEvent(new CustomEvent(SchedulerEvents.EVENT_REMINDER, { detail }));
