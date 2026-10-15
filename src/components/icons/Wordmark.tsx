import React from "react";

/** Meet Assist mark: a speech bubble holding a three-bar level. */
export const Mark: React.FC<{ className?: string }> = ({ className = "" }) => (
  <svg viewBox="0 0 20 20" className={className} aria-hidden="true" fill="none">
    <path
      d="M4 2.5h12A2.5 2.5 0 0 1 18.5 5v7A2.5 2.5 0 0 1 16 14.5H9.2L5.4 17.6a.6.6 0 0 1-1-.47V14.5H4A2.5 2.5 0 0 1 1.5 12V5A2.5 2.5 0 0 1 4 2.5Z"
      fill="currentColor"
      opacity="0.16"
    />
    <path
      d="M7 7v3.5M10 5.5V12M13 7.5V10"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
    />
  </svg>
);

/** The product name is not translated. */
const NAME = "Meet Assist";

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
