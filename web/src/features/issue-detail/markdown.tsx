import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

// Comments are rendered without raw HTML; links open in a new tab.
export default function Markdown({ text }: { text: string }) {
  return (
    <div className="prose-sm max-w-none text-[13px] leading-relaxed [&_.contains-task-list]:list-none [&_.contains-task-list]:pl-1 [&_a]:text-brand-text [&_a]:underline [&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:py-px [&_code]:font-mono [&_code]:text-[12px] [&_input]:mr-1.5 [&_input]:align-middle [&_li]:my-0.5 [&_ol]:list-decimal [&_ol]:pl-5 [&_p]:my-1 [&_pre]:overflow-x-auto [&_pre]:rounded-md [&_pre]:bg-code-bg [&_pre]:p-2 [&_table]:text-xs [&_td]:border [&_td]:px-2 [&_th]:border [&_th]:px-2 [&_ul]:list-disc [&_ul]:pl-5">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noreferrer noopener" /> }}>
        {text}
      </ReactMarkdown>
    </div>
  )
}
