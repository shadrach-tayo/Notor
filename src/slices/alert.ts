import { Schema$Event } from "@/services/api/googleCalendar";
import { PayloadAction, createSlice } from "@reduxjs/toolkit";

type AlertState = {
  alert?: Schema$Event | undefined;
};

const initialState: AlertState = {};

const alert = createSlice({
  name: "alert",
  initialState,
  reducers: {
    setAlert: (state, { payload }: PayloadAction<Schema$Event>) => {
      state.alert = payload;
    },
    removeAlert: (state) => {
      state.alert = undefined;
    },
  },
});

export default alert.reducer;
export const { setAlert } = alert.actions;
