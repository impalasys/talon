import * as React from "react";

declare module "react" {
  interface Attributes {
    children?: React.ReactNode;
    color?: string;
    variant?: string;
    size?: string;
    className?: string;
  }
}
