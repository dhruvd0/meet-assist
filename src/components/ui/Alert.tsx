import React from "react";
import { AlertCircle, AlertTriangle, Info, CheckCircle } from "lucide-react";

type AlertVariant = "error" | "warning" | "info" | "success";

interface AlertProps {
  variant?: AlertVariant;
  /** When true, removes rounded corners for use inside containers */
  contained?: boolean;
  children: React.ReactNode;
  className?: string;
}

const variantStyles: Record<
  AlertVariant,
  { container: string; icon: string; text: string }
> = {
  error: {
    container: "bg-error/8 border border-error/20",
    icon: "text-error",
    text: "text-text",
  },
  warning: {
    container: "bg-warning/8 border border-warning/20",
    icon: "text-warning",
    text: "text-text",
  },
  info: {
    container: "bg-surface border border-line",
    icon: "text-accent",
    text: "text-muted",
  },
  success: {
    container: "bg-ok/8 border border-ok/20",
    icon: "text-ok",
    text: "text-text",
  },
};

const variantIcons: Record<AlertVariant, React.ElementType> = {
  error: AlertCircle,
  warning: AlertTriangle,
  info: Info,
  success: CheckCircle,
};

export const Alert: React.FC<AlertProps> = ({
  variant = "error",
  contained = false,
  children,
  className = "",
}) => {
  const styles = variantStyles[variant];
  const Icon = variantIcons[variant];

  return (
    <div
      className={`flex items-start gap-2.5 px-3.5 py-3 ${styles.container} ${contained ? "" : "rounded-xl"} ${className}`}
    >
      <Icon className={`w-4 h-4 shrink-0 mt-[3px] ${styles.icon}`} />
      <p className={`text-sm ${styles.text}`}>{children}</p>
    </div>
  );
};
