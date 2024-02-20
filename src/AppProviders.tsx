"use client";

import { PropsWithChildren } from "react";
import { Provider } from "react-redux";
import { persistor, store } from "@/store/store";
import { PersistGate } from "redux-persist/integration/react";
import EventsProvider from "./context/EventsProvider";

export default function AppProviders(props: PropsWithChildren<unknown>) {
  return (
    <Provider store={store}>
      <PersistGate persistor={persistor}>
        <EventsProvider>{props.children}</EventsProvider>
      </PersistGate>
    </Provider>
  );
}
export function Providers(props: PropsWithChildren<unknown>) {
  return (
    <Provider store={store}>
      <PersistGate persistor={persistor}>{props.children}</PersistGate>
    </Provider>
  );
}
