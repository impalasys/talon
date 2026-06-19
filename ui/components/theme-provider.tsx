"use client";

import * as React from "react";

export function ThemeProvider({
  children,
}: React.PropsWithChildren) {
  React.useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const root = document.documentElement;

    const applySystemTheme = () => {
      const isDark = media.matches;
      root.classList.toggle("dark", isDark);
      root.classList.toggle("light", !isDark);
      root.style.colorScheme = isDark ? "dark" : "light";
    };

    applySystemTheme();
    media.addEventListener("change", applySystemTheme);

    return () => {
      media.removeEventListener("change", applySystemTheme);
    };
  }, []);

  return <>{children}</>;
}
