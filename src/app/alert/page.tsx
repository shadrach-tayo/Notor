"use client";
import { Providers } from "@/AppProviders";
import { Button } from "@/components/ui/button";
import { useAlert, useUser } from "@/slices/hooks";
import { invoke } from "@tauri-apps/api/tauri";
import { useCallback, useEffect, useMemo, useState } from "react";
import { formatRelative, formatDistance } from "date-fns";
import { cn } from "@/lib/utils";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import {
  Schema$Event,
  Schema$EventAttendee,
} from "@/services/api/googleCalendar";
import { open } from "@tauri-apps/api/shell";
import { GoogleMeetIcon } from "@/components/icons/icons";

function AlertInfo() {
  const alert = useAlert();
  const [timeLabel, setTimeLabel] = useState<string>();
  const [runningLate, setRunningLate] = useState(false);

  const closeAlert = async () => {
    let res = await invoke("dismiss_alert");
    console.log("DISMISS ALERT", res);
  };

  console.log({ alert });
  function toggleFullscreen() {
    // let video = document.querySelector("video");
    // let main = document.querySelector("main");
    // let html = document.querySelector("html");
    // console.log({ video, main });
    // console.log(
    //   main?.requestFullscreen,
    //   video?.requestFullscreen,
    //   document.fullscreenElement,
    //   document.fullscreenEnabled
    // );
    // if (!document.fullscreenElement && html && html.requestFullscreen) {
    //   html.requestFullscreen().catch((err) => {
    //     // alert(
    //     //   `Error attempting to enable fullscreen mode: ${err.message} (${err.name})`
    //     // );
    //     console.log("full screen error", err);
    //   });
    // } else {
    //   console.log("full screen not supported");
    //   // document?.exitFullscreen();
    // }
  }

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
    toggleFullscreen();
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
    return { start: startTime.substring(0, 5), end: endTime.substring(0, 5) };
  }, [alert]);

  // const startTime = alert?.start?.date || alert?.start?.dateTime;
  let now = Date.now();

  console.log(now, alert);

  return (
    <main
      data-tauri-drag-region
      className="bg-background flex h-full min-h-screen flex-col items-center justify-center rounded-md p-24 backdrop-blur-md space-y-2"
    >
      <h1 className="text-5xl line-clamp-1">
        {alert?.summary ?? "No Alert to show!"}
      </h1>
      <p className="text-2xl text-purple-600">
        {time.start} - {time.end}
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
          onClick={() => {
            open(alert.hangoutLink!);
            closeAlert();
          }}
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
  alert,
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

export default function Alert() {
  return (
    <Providers>
      <AlertInfo />
    </Providers>
  );
}
