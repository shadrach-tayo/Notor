"use client";
import { Providers } from "@/AppProviders";
import { Button } from "@/components/ui/button";
import { useAlert, useUser } from "@/slices/hooks";
import { invoke } from "@tauri-apps/api/tauri";
import { useCallback, useEffect, useMemo, useState } from "react";
import { formatDistance } from "date-fns";
import { cn } from "@/lib/utils";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import {
  Schema$Event,
  Schema$EventAttendee,
} from "@/services/api/googleCalendar";
import { open } from "@tauri-apps/api/shell";
import { GoogleMeetIcon, ZoomMeetIcon } from "@/components/icons/icons";

function AlertInfo() {
  const alert = useAlert();
  const [timeLabel, setTimeLabel] = useState<string>();
  const [runningLate, setRunningLate] = useState(false);

  const closeAlert = async () => {
    let res = await invoke("dismiss_alert");
    console.log("DISMISS ALERT", res);
  };

  const calculateTimeLabel = useCallback(() => {
    if (!alert) return;

    const now = new Date().getTime();
    const start = new Date(
      alert.start?.date || alert.start?.dateTime || ""
    ).getTime();
    const end = new Date(
      alert.end?.date || alert.end?.dateTime || ""
    ).getTime();

    const startTime = alert?.start?.date || alert?.start?.dateTime;

    if (!startTime) return;

    const timeDistance = formatDistance(startTime, now);

    if (now > start && now < end) {
      setTimeLabel(`This event started ${timeDistance} ago`);
      setRunningLate(true);
    } else if (now < start) {
      setTimeLabel(`This event will start in ${timeDistance}`);
    } else if (now > end) {
      setTimeLabel(`This event ended ${timeDistance} ago`);
    }
  }, [alert]);

  useEffect(() => {
    const intervalId = setInterval(() => calculateTimeLabel(), 1000);
    return () => clearInterval(intervalId);
  }, [calculateTimeLabel]);

  const time = useMemo(() => {
    if (!alert) return { start: "00:00", end: "00:00" };
    const start = new Date(
      alert.start?.date || alert.start?.dateTime || Date.now()
    );
    let startTime = start.toLocaleTimeString();
    const end = new Date(alert.end?.date || alert.end?.dateTime || Date.now());
    let endTime = end.toLocaleTimeString();
    // console.log({ endTime, end, hour: end.getHours() })
    const endTimeSuffix = end.getHours() >= 12 ? "PM" : "AM";
    return {
      start: startTime.substring(0, 5),
      end: endTime.substring(0, 5),
      endTimeSuffix,
    };
  }, [alert]);

  let now = Date.now();

  const onHandleJoin = async (link: string) => {
    await open(link);
    await closeAlert();
  };
  if (!alert) return;

  return (
    <main className="bg-background flex h-full min-h-screen flex-col items-center justify-center rounded-md p-24 backdrop-blur-md gap-3">
      <h1 className="text-5xl">{alert?.summary ?? "No Alert to show!"}</h1>
      <p className="text-2xl text-purple-600">
        {time.start} - {time.end} {time.endTimeSuffix}
      </p>
      {timeLabel && (
        <span className={cn(runningLate && "text-red-500", "text-sm")}>
          {timeLabel}
        </span>
      )}
      <div className="flex items-center justify-start gap-2 py-4">
        {alert?.attendees?.map((attendee, idx) => (
          <Attendee key={idx} attendee={attendee} />
        ))}
      </div>
      {alert?.hangoutLink ? (
        <GoogleMeetButton
          alert={alert}
          onClick={() => onHandleJoin(alert.hangoutLink!)}
        />
      ) : isZoomMeeting(alert) ? (
        <ZoomMeetButton
          alert={alert}
          onClick={() => onHandleJoin(getZoomLink(alert))}
        />
      ) : (
        <Button
          variant="ghost"
          className="bg-primary-foreground hover:bg-secondary"
          onClick={closeAlert}
        >
          Ok
        </Button>
      )}
    </main>
  );
}

const Attendee = ({ attendee }: { attendee: Schema$EventAttendee }) => {
  const user = useUser();
  const isOrganizer = user.email === attendee.email;
  return (
    <Avatar className="border border-white backdrop:shadow-md shadow-white w-8 h-8 cursor-pointer">
      <AvatarImage
        src={isOrganizer ? user.picture : ""}
        alt={attendee.displayName || attendee.email || ""}
      />
      <AvatarFallback className="capitalize">
        {attendee.displayName || attendee.email?.substring(0, 1)}
      </AvatarFallback>
    </Avatar>
  );
};

const GoogleMeetButton = ({
  onClick,
}: {
  alert: Schema$Event;
  onClick: () => void;
}) => {
  return (
    <Button
      variant="ghost"
      className="bg-primary-foreground hover:bg-secondary text-[11px] rounded-lg"
      onClick={onClick}
    >
      <span className="mr-1">Join Google meet meeting</span>{" "}
      <GoogleMeetIcon className="w-6 h-6 fill-transparent" style={{}} />
    </Button>
  );
};

const isZoomMeeting = (event: Schema$Event) => {
  const matches = event?.location?.match(/http.*.zoom.us\S*/gm);
  return !event?.hangoutLink && !!matches;
};
const getZoomLink = (event: Schema$Event) => {
  const matches = event?.location?.match(/http.*.zoom.us\S*/gm);
  return matches?.[0] ?? "";
};

const ZoomMeetButton = ({
  onClick,
}: {
  alert: Schema$Event;
  onClick: () => void;
}) => {
  return (
    <Button
      variant="ghost"
      className="bg-primary-foreground hover:bg-secondary text-[11px] rounded-lg bg-blue-500 hover:bg-blue-500 hover:border-blue-300"
      onClick={onClick}
    >
      <ZoomMeetIcon className="w-6 h-6 fill-transparent stroke-blue mr-2 " />
      <span>Join Zoom meeting</span>{" "}
    </Button>
  );
};

export default function Alert() {
  return (
    <Providers>
      <AlertInfo />
    </Providers>
  );
}
