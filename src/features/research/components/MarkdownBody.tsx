import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"

interface MarkdownBodyProps {
  markdown: string
}

/**
 * Renders an LLM-authored research-note body. Uses GFM (tables,
 * strikethrough, task lists) because note authors lean on tables for
 * price-action history. No raw HTML / `dangerouslySetInnerHTML` —
 * react-markdown's default sanitizer is sufficient for this trusted
 * source of content (the body is written via the MCP tool and never
 * reflected from a user-controlled web surface), but we still avoid
 * the raw passthrough out of habit.
 *
 * Element-level styling is inline rather than a `prose` plugin
 * because the project doesn't ship `@tailwindcss/typography` and the
 * dark-themed UI wants its own colors anyway.
 */
export function MarkdownBody({ markdown }: MarkdownBodyProps) {
  return (
    <div className="text-foreground/90 mt-2 text-sm leading-relaxed">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          h1: ({ children }) => (
            <h1 className="text-foreground mt-3 mb-2 text-base font-semibold">{children}</h1>
          ),
          h2: ({ children }) => (
            <h2 className="text-foreground mt-3 mb-2 text-sm font-semibold tracking-wide uppercase">
              {children}
            </h2>
          ),
          h3: ({ children }) => (
            <h3 className="text-foreground mt-2 mb-1 text-sm font-semibold">{children}</h3>
          ),
          p: ({ children }) => <p className="my-2">{children}</p>,
          ul: ({ children }) => <ul className="my-2 list-disc space-y-1 pl-5">{children}</ul>,
          ol: ({ children }) => <ol className="my-2 list-decimal space-y-1 pl-5">{children}</ol>,
          li: ({ children }) => <li className="marker:text-muted-foreground">{children}</li>,
          strong: ({ children }) => (
            <strong className="text-foreground font-semibold">{children}</strong>
          ),
          em: ({ children }) => <em className="italic">{children}</em>,
          a: ({ children, href }) => (
            <a
              href={href}
              target="_blank"
              rel="noopener noreferrer"
              className="text-primary underline-offset-2 hover:underline"
            >
              {children}
            </a>
          ),
          code: ({ children }) => (
            <code className="bg-muted text-foreground rounded px-1 py-0.5 font-mono text-xs">
              {children}
            </code>
          ),
          pre: ({ children }) => (
            <pre className="bg-muted my-2 overflow-x-auto rounded p-2 font-mono text-xs">
              {children}
            </pre>
          ),
          blockquote: ({ children }) => (
            <blockquote className="text-muted-foreground my-2 border-l-2 border-amber-400/40 pl-3 italic">
              {children}
            </blockquote>
          ),
          hr: () => <hr className="border-border my-3" />,
          table: ({ children }) => (
            <div className="my-2 overflow-x-auto">
              <table className="border-border w-full border-collapse border text-xs">
                {children}
              </table>
            </div>
          ),
          thead: ({ children }) => <thead className="bg-muted/50">{children}</thead>,
          th: ({ children }) => (
            <th className="border-border text-foreground border px-2 py-1 text-left font-semibold">
              {children}
            </th>
          ),
          td: ({ children }) => (
            <td className="border-border border px-2 py-1 align-top">{children}</td>
          ),
        }}
      >
        {markdown}
      </ReactMarkdown>
    </div>
  )
}
