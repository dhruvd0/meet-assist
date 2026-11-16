import React from "react";

/**
 * The rounded bar that floats over the page (the in-app live dock) or sits
 * alone in the always-on-top window (`floating`).
 */
export const PillShell: React.FC<{
  floating: boolean;
  label: string;
  children: React.ReactNode;
}> = ({ floating, label, children }) => (
  <div
    className={
      floating
        ? "h-screen w-screen flex items-center justify-center px-2"
        : "pointer-events-none absolute inset-x-0 bottom-5 flex justify-center px-6 z-20"
    }
  >
    <div
      role="region"
      aria-label={label}
      className={`pointer-events-auto flex items-center gap-1 h-12 ps-2 pe-1.5 rounded-full bg-surface/95 border border-line-strong backdrop-blur-md max-w-full ${
        floating
          ? "shadow-[0_4px_12px_-4px_rgb(0_0_0/0.4)]"
          : "shadow-[0_16px_40px_-12px_rgb(0_0_0/0.45),0_2px_6px_-2px_rgb(0_0_0/0.2)]"
      }`}
    >
      {children}
    </div>
  </div>
);
