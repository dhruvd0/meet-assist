import React from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  variant?: "default" | "compact";
}

export const Input: React.FC<InputProps> = ({
  className = "",
  variant = "default",
  disabled,
  ...props
}) => {
  const baseClasses =
    "text-sm bg-surface border border-line rounded-lg text-start text-text placeholder:text-faint transition-[border-color,background-color] duration-150";

  const interactiveClasses = disabled
    ? "opacity-60 cursor-not-allowed"
    : "hover:border-line-strong focus:outline-none focus:border-accent/70";

  const variantClasses = {
    default: "h-9 px-3",
    compact: "h-8 px-2.5",
  } as const;

  return (
    <input
      className={`${baseClasses} ${variantClasses[variant]} ${interactiveClasses} ${className}`}
      disabled={disabled}
      {...props}
    />
  );
};
