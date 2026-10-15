import React from "react";

interface BadgeProps {
  children: React.ReactNode;
  variant?: "primary" | "success" | "secondary";
  className?: string;
}

const Badge: React.FC<BadgeProps> = ({
  children,
  variant = "primary",
  className = "",
}) => {
  const variantClasses = {
    primary: "bg-accent/15 text-accent",
    success: "bg-ok/15 text-ok",
    secondary: "bg-surface-hover text-muted",
  };

  return (
    <span
      className={`inline-flex items-center gap-1 h-5 px-2 rounded-full text-[11px] font-medium whitespace-nowrap ${variantClasses[variant]} ${className}`}
    >
      {children}
    </span>
  );
};

export default Badge;
