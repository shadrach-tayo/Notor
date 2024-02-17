import { Schema$Event } from "@/services/api/googleCalendar";
import { dispatchEventNotification } from "./customEvents";

type ScheduledEvent = {
  event: Schema$Event;
  startTime: number;
};

const delayMap = {
  string: function dateTimeStringToUnixTs(dateTime: string) {
    return new Date(dateTime).getTime() / 1000;
  },

  number: function numberInSecsToUnixTs(delay: number) {
    return new Date().getTime() / 1000 + delay;
  },

  obj: function dateToUnixTs(date: Date) {
    return date.getTime() / 1000;
  },
};

export class Scheduler {
  pendingEvents: Map<string, ScheduledEvent> = new Map();
  scheduledEvents: Map<string, ScheduledEvent> = new Map();

  static instance: Scheduler;

  constructor() {}

  static getInstance(): Scheduler {
    if (!this.instance) {
      console.log("[SCHEDULER]::INIT NEW SCHEDULAR INSTANCE");
      this.instance = new Scheduler();
    }
    return this.instance;
  }

  scheduleEvent(event: Schema$Event, options?: { delayInSeconds: number }) {
    if (!event.id) return;
    const evt = this.pendingEvents.get(event.id ?? "");
    console.log("[SCHEDULER]::EXISTING EVENT", evt?.event.summary);
    const scheduleAt =
      options?.delayInSeconds ||
      delayMap.string?.(event.start?.date || event.start?.dateTime || "");

    console.log("[SCHEDULER]::SCHEDUELE AT", event.summary, scheduleAt);
    if (Number.isNaN(scheduleAt)) return;

    const delayInSeconds = scheduleAt - new Date().getTime() / 1000;

    console.log("[SCHEDULER]::delayInSeconds", event.summary, delayInSeconds);
    if (delayInSeconds < 0) throw new Error("CANNOT EXECUTE PAST EVENT");

    const startTime = new Date(new Date().getTime() + delayInSeconds * 1000);
    if (evt && evt.startTime === startTime.getTime()) {
      console.log("[SCHEDULER]::SKIP SCHEDULING EXISTING EVENT", event.summary);
      return;
    }

    const delayInMs = delayInSeconds * 1000;
    console.log("[SCHEDULER]::executing in ", delayInMs, startTime);

    this.pendingEvents.set(event.id, { event, startTime: startTime.getTime() });
    const timoutId = setTimeout(() => {
      console.log("[SCHEDULER]::", event.summary + " Time is now");
      this.pendingEvents.delete(event.id!);
      this.scheduledEvents.set(event.id!, {
        event,
        startTime: startTime.getTime(),
      });
      dispatchEventNotification({ event });
    }, delayInSeconds * 1000);

    return (function (id) {
      return function () {
        clearTimeout(id);
      };
    })(timoutId);
  }
}

export const scheduler = Scheduler.getInstance();
