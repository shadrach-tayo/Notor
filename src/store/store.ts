import { googleApi } from "@/services/api";
import authReducer from "@/slices/authSlice";
import calendarsReducer from "@/slices/calendars";
import { combineReducers, configureStore } from "@reduxjs/toolkit";
import {
  PersistedState,
  createMigrate,
  createTransform,
  persistReducer,
  persistStore,
} from "redux-persist";
import storage from "redux-persist/es/storage";

const migrations = {
  1: (state: PersistedState) => {
    console.log("migrate", state);
    return {} as PersistedState;
  },
};

const rootReducer = combineReducers({
  auth: authReducer,
  calendars: calendarsReducer,
  [googleApi.reducerPath]: googleApi.reducer,
});

const nestedWhitelist = createTransform(
  null,
  (state: PersistedState) => state,
  { whitelist: ["auth"] }
);

const rootPersistConfig = {
  key: "root",
  keyPrefix: "notor/",
  version: 1,
  storage,
  migrate: createMigrate(migrations),
  whitelist: ["auth"],
  transforms: [nestedWhitelist],
};

const persistedReducer = persistReducer(
  rootPersistConfig,
  rootReducer
) as typeof rootReducer;

export const store = configureStore({
  reducer: persistedReducer,
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware({ serializableCheck: false }).concat([
      googleApi.middleware,
    ]),
  devTools: process.env.NODE_ENV !== "production",
});

export const persistor = persistStore(store);

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
