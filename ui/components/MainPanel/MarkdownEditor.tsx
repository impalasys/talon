import CodeMirror from '@uiw/react-codemirror';
import { markdown } from '@codemirror/lang-markdown';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { oneDark } from '@codemirror/theme-one-dark';

type MarkdownEditorProps = {
  value: string;
  className?: string;
  language?: 'markdown' | 'text';
};

export function MarkdownEditor({ value, className, language = 'markdown' }: MarkdownEditorProps) {
  return (
    <div className={`sightline-markdown-editor ${className || ''} overflow-hidden`}>
      <CodeMirror
        className="h-full"
        value={value}
        height="100%"
        basicSetup={{
          foldGutter: true,
          highlightActiveLine: false,
          highlightActiveLineGutter: false,
        }}
        editable={false}
        theme={typeof document !== 'undefined' && document.documentElement.classList.contains('dark') ? oneDark : 'light'}
        extensions={[
          ...(language === 'markdown' ? [markdown()] : []),
          EditorState.readOnly.of(true),
          EditorView.lineWrapping,
          EditorView.theme({
            '&': {
              height: '100%',
              background: 'transparent',
              fontSize: '13px',
            },
            '.cm-editor': {
              height: '100%',
            },
            '.cm-editor .cm-scroller': {
              overflow: 'auto',
            },
            '.cm-scroller': {
              fontFamily:
                'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
            },
            '.cm-content': {
              padding: '0.75rem 0 4.5rem',
            },
            '.cm-gutters': {
              background: 'transparent',
              borderRightColor: 'var(--border)',
            },
          }),
        ]}
      />
    </div>
  );
}
