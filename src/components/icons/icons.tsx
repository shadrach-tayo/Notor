import {ElementType, SVGProps} from "react";

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
        <path d="M166 106v90h-90" fill="#ea4335"/>
        <path d="M166 106v90h120v62l90-73v-49q0-30-30-30" fill="#ffba00"/>
        <path d="M164 406v-90h122v-60l90 71v49q0 30-30 30" fill="#00ac47"/>
        <path d="M286 256l90-73v146" fill="#00832d"/>
        <path
            d="M376 183l42-34c9-7 18-7 18 7v200c0 14-9 14-18 7l-42-34"
            fill="#00ac47"
        />
        <path d="M76 314v62q0 30 30 30h60v-92" fill="#0066da"/>
        <path d="M76 196h90v120h-90" fill="#2684fc"/>
    </svg>
);

export const ZoomMeetIcon: ElementType<SVGProps<SVGElement>> = ({
                                                                    ref,
                                                                    ...props
                                                                }) => (
    <svg xmlns="http://www.w3.org/2000/svg" enableBackground="new 0 0 100 100" viewBox="0 0 100 100"
         id="zoom" {...props}>
        <g>
            <path fill="#2d8cff" d="M50,2.5C23.766,2.5,2.5,23.823,2.5,50.126c2.502,63.175,92.507,63.157,95,0C97.5,23.823,76.233,2.5,50,2.5
				z"></path>
            <path fill="#f1f1f1" d="M78.285,63.557c-0.051,2.506-2.059,3.352-4.005,1.965c-3.629-2.54-7.233-5.115-10.851-7.669
				c0.009,1.182-0.007,3.966-0.001,5.177c-0.001,2.246-1.117,3.339-3.41,3.34l-19.536,0.002c-3.31,0.001-6.619,0.002-9.929-0.002
				c-6.257-0.007-10.462-4.106-10.464-10.201l0-19.205c0-2.278,1.081-3.34,3.4-3.34c7.951,0.151,21.843,0.017,29.638,0.003
				c5.525,0.006,9.522,3.442,10.19,8.52c3.656-2.58,7.297-5.181,10.963-7.748c1.691-1.227,3.917-0.833,4.005,1.966
				C78.309,45.151,78.338,54.754,78.285,63.557z"></path>
        </g>
    </svg>
);
