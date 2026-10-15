import React from "react";
import ReactMarkdown, { type Components } from "react-markdown";

// The assistant writes notes as plain lines: "Decisions:" followed by "- item".
// Promote those short label lines to headings so they read as sections.
const promoteLabels = (text: string) =>
  text.replace(/^([^\n:#-][^\n:]{0,48}):[ \t]*$/gm, "### $1");

const components: Components = {
  h1: ({ children }) => (
    <h3 className="mt-6 first:mt-0 mb-1.5 text-[14px] font-semibold text-text">
      {children}
    </h3>
  ),
  h2: ({ children }) => (
    <h3 className="mt-6 first:mt-0 mb-1.5 text-[14px] font-semibold text-text">
      {children}
    </h3>
  ),
  h3: ({ children }) => (
    <h3 className="mt-6 first:mt-0 mb-1.5 text-[14px] font-semibold text-text">
      {children}
    </h3>
  ),
  p: ({ children }) => (
    <p className="my-2 first:mt-0 text-[15px] leading-7 text-text">
      {children}
    </p>
  ),
  ul: ({ children }) => <ul className="my-1 space-y-1">{children}</ul>,
  ol: ({ children }) => (
    <ol className="my-1 ps-5 list-decimal space-y-1 marker:text-faint">
      {children}
    </ol>
  ),
  li: ({ children }) => (
    <li className="relative ps-5 text-[15px] leading-7 text-text before:absolute before:start-1.5 before:top-[11.5px] before:w-[5px] before:h-[5px] before:rounded-full before:bg-faint/70 [ol>&]:ps-0 [ol>&]:before:hidden">
      {children}
    </li>
  ),
  strong: ({ children }) => (
    <strong className="font-semibold text-text">{children}</strong>
  ),
  code: ({ children }) => (
    <code className="font-mono text-[13px] px-1 py-0.5 rounded bg-surface border border-line">
      {children}
    </code>
  ),
};

const allowedElements = [
  "h1",
  "h2",
  "h3",
  "p",
  "ul",
  "ol",
  "li",
  "strong",
  "em",
  "code",
  "br",
];

/** Assistant-written notes, set like a document. Links and images are dropped. */
export const NotesMarkdown: React.FC<{ text: string }> = ({ text }) => (
  <div className="select-text">
    <ReactMarkdown
      components={components}
      allowedElements={allowedElements}
      unwrapDisallowed
    >
      {promoteLabels(text)}
    </ReactMarkdown>
  </div>
);
