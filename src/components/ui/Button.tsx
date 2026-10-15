import React from "react";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?:
    | "primary"
    | "primary-soft"
    | "secondary"
    | "warning"
    | "danger"
    | "danger-ghost"
    | "ghost";
  size?: "sm" | "md" | "lg";
}

export const Button: React.FC<ButtonProps> = ({
  children,
  className = "",
  variant = "primary",
  size = "md",
  ...props
}) => {
  const baseClasses =
    "inline-flex items-center justify-center gap-1.5 font-medium rounded-lg border transition-[background-color,border-color,color,opacity] duration-150 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer whitespace-nowrap";

  // Primary is the inverted neutral (t3/Granola); the accent is kept for
  // live state, focus, and selection rather than for every action.
  const variantClasses = {
    primary:
      "text-background bg-text border-text hover:bg-text/85 hover:border-text/85",
    "primary-soft":
      "text-accent bg-accent/12 border-transparent hover:bg-accent/20",
    secondary:
      "text-text bg-surface border-line hover:bg-surface-hover hover:border-line-strong",
    // Secondary's resting look with warning hover, for buttons sitting on
    // warning surfaces.
    warning:
      "text-text bg-surface border-line hover:bg-warning/15 hover:border-warning",
    danger:
      "text-error bg-error/10 border-error/25 hover:bg-error/18 hover:border-error/40",
    "danger-ghost":
      "text-faint border-transparent hover:text-error hover:bg-error/10",
    ghost:
      "text-muted border-transparent hover:text-text hover:bg-surface-hover",
  };

  const sizeClasses = {
    sm: "h-7 px-2.5 text-xs",
    md: "h-8 px-3.5 text-sm",
    lg: "h-10 px-4 text-sm",
  };

  return (
    <button
      className={`${baseClasses} ${variantClasses[variant]} ${sizeClasses[size]} ${className}`}
      {...props}
    >
      {children}
    </button>
  );
};
