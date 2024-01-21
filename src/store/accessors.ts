import { AppDispatch, RootState } from "./store";
import { TypedUseSelectorHook, useDispatch, useSelector } from "react-redux";

export const useSetter = () => useDispatch<AppDispatch>();
export const useGetter: TypedUseSelectorHook<RootState> = useSelector;
