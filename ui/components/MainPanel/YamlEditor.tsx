import CodeMirror from '@uiw/react-codemirror';
import { yaml } from '@codemirror/lang-yaml';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { oneDark } from '@codemirror/theme-one-dark';

type YamlEditorProps = {
  value: string;
  className?: string;
};

export function YamlEditor({ value, className }: YamlEditorProps) {
  return (
    <div className={`sightline-yaml-editor ${className || ''} overflow-hidden`}>
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
          yaml(),
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
              padding: '0.75rem 0',
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
