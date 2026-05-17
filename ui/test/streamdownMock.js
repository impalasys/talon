const React = require('react');

function renderInline(text) {
  return text;
}

function Streamdown({ children }) {
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
            React.createElement('li', { key: itemIndex }, renderInline(line.slice(2))),
          ),
        );
      }

      return React.createElement('p', { key: index }, renderInline(trimmed));
    }),
  );
}

module.exports = { Streamdown };
