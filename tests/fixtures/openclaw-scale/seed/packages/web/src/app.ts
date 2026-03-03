import { pluginSdkValue } from "openclaw/plugin-sdk";
import { webValue } from "./barrel.js";
import type { FeatureConfig } from "../../../extensions/alpha/src/types.js";

const loadLazyInternal = () => import("./lazy.js");

export const appValue = pluginSdkValue + webValue;
export type AppFeatureConfig = FeatureConfig;

export async function loadLazy() {
  return loadLazyInternal();
}
