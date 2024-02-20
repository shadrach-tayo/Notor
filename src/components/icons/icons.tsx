import { ElementType, SVGProps } from "react";

export const GoogleMeetIcon: ElementType<SVGProps<SVGElement>> = ({
  ref,
  ...props
}) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    aria-label="Google Meet"
    role="img"
    viewBox="0 0 512 512"
    {...props}
  >
    <rect
      width={props.height || "512"}
      height={props.width || "512"}
      rx="15%"
    />
    <path d="M166 106v90h-90" fill="#ea4335" />
    <path d="M166 106v90h120v62l90-73v-49q0-30-30-30" fill="#ffba00" />
    <path d="M164 406v-90h122v-60l90 71v49q0 30-30 30" fill="#00ac47" />
    <path d="M286 256l90-73v146" fill="#00832d" />
    <path
      d="M376 183l42-34c9-7 18-7 18 7v200c0 14-9 14-18 7l-42-34"
      fill="#00ac47"
    />
    <path d="M76 314v62q0 30 30 30h60v-92" fill="#0066da" />
    <path d="M76 196h90v120h-90" fill="#2684fc" />
  </svg>
);

export const ZoomMeetIcon: ElementType<SVGProps<SVGElement>> = ({
  ref,
  ...props
}) => (
  <svg
    width="800px"
    height="800px"
    viewBox="0 0 192 192"
    xmlns="http://www.w3.org/2000/svg"
    {...props}
  >
    <path
      stroke="#000000"
      stroke-linecap="round"
      stroke-linejoin="round"
      stroke-width="11.997"
      d="M16.869 60.973v53.832c.048 12.173 10.87 21.965 24.072 21.925h85.406c2.42 0 4.385-1.797 4.385-3.978V78.92c-.064-12.164-10.887-21.965-24.073-21.917H21.237c-2.412 0-4.368 1.79-4.368 3.97zm119.294 21.006 35.27-23.666c3.06-2.332 5.432-1.749 5.432 2.468v72.171c0 4.8-2.9 4.217-5.432 2.468l-35.27-23.618V81.98z"
    />
  </svg>
);
