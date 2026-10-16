import React from "react";

/** CASE mark: two conversational strokes and a private signal. */
export const Mark: React.FC<{ className?: string }> = ({ className = "" }) => (
  <svg viewBox="8 5 50 50" className={className} aria-hidden="true" fill="none">
    <path
      d="M18 42V26c0-9 6-14 14-14M34 42V30c0-6 4-10 10-10"
      stroke="currentColor"
      strokeWidth="9"
      strokeLinecap="round"
    />
    <circle cx="48" cy="47" r="5" fill="currentColor" />
  </svg>
);

/** The product name is not translated. */
const NAME = "CASE";

/** Product name set beside the mark. */
export const Wordmark: React.FC<{ className?: string; size?: "sm" | "lg" }> = ({
  className = "",
  size = "sm",
}) => (
  <span
    className={`inline-flex items-center gap-2 text-text ${className}`}
    aria-label={NAME}
  >
    <Mark
      className={`${size === "lg" ? "w-8 h-8" : "w-5 h-5"} text-accent shrink-0`}
    />
    <span
      className={`font-serif tracking-[-0.01em] ${size === "lg" ? "text-3xl" : "text-[17px]"}`}
    >
      {NAME}
    </span>
  </span>
);
