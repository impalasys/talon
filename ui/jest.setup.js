require('@testing-library/jest-dom');
const { TextDecoder, TextEncoder } = require('util');

global.IS_REACT_ACT_ENVIRONMENT = true;

if (!global.TextEncoder) {
  global.TextEncoder = TextEncoder;
}

if (!global.TextDecoder) {
  global.TextDecoder = TextDecoder;
}

if (!global.fetch) {
  global.fetch = jest.fn();
}

if (!global.HTMLElement.prototype.scrollIntoView) {
  global.HTMLElement.prototype.scrollIntoView = jest.fn();
}

const originalConsoleError = console.error.bind(console);
global.__talonReactActWarnings = [];
console.error = (...args) => {
  const message = args.map((arg) => String(arg)).join(' ');
  if (message.includes('not wrapped in act')) {
    global.__talonReactActWarnings.push(message);
    return;
  }
  originalConsoleError(...args);
};
