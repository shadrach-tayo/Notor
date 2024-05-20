import { useCalendarListQuery } from "@/services/api/googleCalendar";
// import { GoogleAuthToken } from "@/slices/authSlice";
import { CalendarIcon, EyeIcon } from "@/components/icons/icons";
import { EyeOffIcon } from "lucide-react";
import { Button } from "./button";
import { AccountPreference } from "@/types/account";

export default function Calendars({
  accessToken,
  onToggleCalendar,
  accountPreferences,
}: {
  accessToken: string;
  onToggleCalendar: (calendar_id: string, hide: boolean) => void;
  accountPreferences?: AccountPreference;
}) {
  const { data: calendars = [], isLoading } = useCalendarListQuery(
    accessToken,
    {
      skip: !accessToken,
      refetchOnFocus: true,
      refetchOnMountOrArgChange: true,
      refetchOnReconnect: true,
    },
  );

  if (!isLoading && calendars.length === 0) return null;

  return (
    <div>
      <h1 className="text-sm mt-2">Calendars</h1>
      {calendars.map((calendar, idx) => {
        const hidden = accountPreferences?.hidden_calendars.some(
          (cal) => cal === calendar.id,
        );
        return (
          <div key={idx} className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <CalendarIcon className="w-5 h-5 text-gray-500 dark:text-gray-400" />
              <p
                className="text-[12px] font-medium truncate w-[200px]"
                title={calendar.description || calendar.id || ""}
              >
                {calendar.description || calendar.id}
              </p>
            </div>
            <Button
              className="rounded-full"
              size="icon"
              variant="ghost"
              onClick={() =>
                hidden
                  ? onToggleCalendar(calendar.id ?? "", false)
                  : onToggleCalendar(calendar?.id ?? "", true)
              }
            >
              {hidden ? (
                <EyeOffIcon className="w-5 h-5 text-gray-500 dark:text-gray-400" />
              ) : (
                <EyeIcon className="w-5 h-5 text-gray-500 dark:text-gray-400" />
              )}
              <span className="sr-only">Toggle visibility</span>
            </Button>
          </div>
        );
      })}
    </div>
  );
}
