require('@testing-library/jest-dom');
const { TextDecoder, TextEncoder } = require('util');

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
