import React, { useLayoutEffect, useRef } from "react";

/**
 * A heading block in a document page (meeting notes, space brief). The faint
 * "#" hangs in the gutter so headings align with the body text.
 */
export const DocSection: React.FC<{
  title: string;
  description?: string;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}> = ({ title, description, action, children, className = "" }) => (
  <section className={`mt-10 ${className}`}>
    <div className="flex items-center justify-between gap-3 min-h-8">
      <h2 className="relative text-[15px] font-semibold text-text">
        <span
          aria-hidden="true"
          className="absolute -start-5 font-normal text-faint/70 select-none"
        >
          #
        </span>
        {title}
      </h2>
      {action}
    </div>
    {description && (
      <p className="text-[13px] text-faint leading-relaxed mb-2">
        {description}
      </p>
    )}
    <div className="mt-1.5">{children}</div>
  </section>
);

/** Borderless textarea that grows with its text, for writing inside a page. */
export const DocTextarea: React.FC<
  React.TextareaHTMLAttributes<HTMLTextAreaElement>
> = ({ className = "", value, ...props }) => {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);
  return (
    <textarea
      ref={ref}
      rows={1}
      value={value}
      className={`w-[calc(100%+1.5rem)] -mx-3 px-3 py-2 rounded-lg bg-transparent resize-none overflow-hidden text-[15px] leading-7 text-text placeholder:text-faint hover:bg-surface focus:bg-surface focus:outline-none transition-colors ${className}`}
      {...props}
    />
  );
};
