import { CalendarEventMap } from "@/lib/customEvents";

declare global {
  interface Window {
    addEventListener<K extends keyof CalendarEventMap>(
      type: K,
      listener: (this: Window, ev: CalendarEventMap[K]) => any,
      options?: boolean | AddEventListenerOptions
    ): void;

    removeEventListener<K extends keyof CalendarEventMap>(
      type: K,
      listener: (this: Window, ev: CalendarEventMap[K]) => any,
      options?: boolean | EventListenerOptions
    ): void;
  }
}
