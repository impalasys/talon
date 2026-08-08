"use client";

import React from "react";
import type { TalonSessionProps } from "./session/TalonSessionTypes";
import { TalonSessionView } from "./session/TalonSessionView";
import { useTalonSessionController } from "./session/useTalonSessionController";

export type * from "./session/TalonSessionTypes";
export type { ResourceViewModel } from "./lib/resourceUris";

/** Public session boundary: compose the controller with its pure presentation shell. */
export function TalonSession(props: TalonSessionProps) {
  return <TalonSessionView {...useTalonSessionController(props)} />;
}

export const TalonCopilot = TalonSession;
