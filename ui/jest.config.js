const customJestConfig = {
  setupFilesAfterEnv: ['<rootDir>/jest.setup.js'],
  testEnvironment: 'jest-environment-jsdom',
  testPathIgnorePatterns: [
    '<rootDir>/e2e/',
    '/node_modules/',
  ],
  moduleNameMapper: {
    '^@/(.*)$': '<rootDir>/$1',
    '^@impalasys/talon-chat$': '<rootDir>/../packages/talon-chat/src/index.ts',
    '^@impalasys/talon-client$': '<rootDir>/test/talonClientMock.js',
    '^streamdown$': '<rootDir>/test/streamdownMock.js',
    '\\.(css|less|scss|sass)$': '<rootDir>/test/styleMock.js',
  },
  transform: {
    '^.+\\.(ts|tsx)$': ['ts-jest', { tsconfig: '<rootDir>/tsconfig.json' }],
  },
  collectCoverage: true,
  collectCoverageFrom: [
    'lib/**/*.{ts,tsx}',
    '../packages/talon-chat/src/**/*.{ts,tsx}',
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

module.exports = customJestConfig;
