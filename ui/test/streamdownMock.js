const React = require('react');

function renderInline(text, components) {
  const link = text.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
  if (link) {
    const Anchor = components?.a || 'a';
    return React.createElement(Anchor, { href: link[2] }, link[1]);
  }
  return text;
}

function Streamdown({ children, components }) {
  const source = typeof children === 'string' ? children : '';
  const blocks = source.split(/\n\n+/);

  return React.createElement(
    React.Fragment,
    null,
    blocks.map((block, index) => {
      const trimmed = block.trim();
      if (!trimmed) return null;

      if (trimmed.startsWith('### ')) {
        return React.createElement('h3', { key: index }, trimmed.slice(4));
      }

      if (trimmed.startsWith('## ')) {
        return React.createElement('h2', { key: index }, trimmed.slice(3));
      }

      if (trimmed.startsWith('# ')) {
        return React.createElement('h1', { key: index }, trimmed.slice(2));
      }

      if (trimmed.split('\n').every((line) => line.startsWith('- '))) {
        return React.createElement(
          'ul',
          { key: index },
          trimmed.split('\n').map((line, itemIndex) =>
          React.createElement('li', { key: itemIndex }, renderInline(line.slice(2), components)),
        ),
      );
    }

      return React.createElement('p', { key: index }, renderInline(trimmed, components));
    }),
  );
}

module.exports = { Streamdown };
