const LOCAL_API_SERVER = "http://127.0.0.1:4876";
const PRODUCTION_API_SERVER = "https://notor-t8pl3.ondigitalocean.app";

export const API_SERVER =
  process.env.NEXT_PUBLIC_API_SERVER ||
  (process.env.NODE_ENV === "development"
    ? LOCAL_API_SERVER
    : PRODUCTION_API_SERVER);
export const RPC_SERVER = process.env.NEXT_PUBLIC_RPC_SERVER || "http://localhost:4875";
