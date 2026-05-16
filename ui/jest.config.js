const nextJest = require('next/jest');

const createJestConfig = nextJest({
  dir: './',
});

const customJestConfig = {
  setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],
  testEnvironment: 'jest-environment-jsdom',
  testPathIgnorePatterns: [
    '<rootDir>/e2e/',
    '/node_modules/',
  ],
  collectCoverage: true,
  collectCoverageFrom: [
    'lib/**/*.{ts,tsx}',
    '!**/*.d.ts',
    '!proto/**',
    '!e2e/**',
  ],
  coveragePathIgnorePatterns: [
    '/node_modules/',
    '<rootDir>/proto/',
    '<rootDir>/e2e/',
  ],
  coverageThreshold: {
    global: {
      branches: 75,
      functions: 75,
      lines: 80,
      statements: 80,
    },
  },
};

module.exports = createJestConfig(customJestConfig);
