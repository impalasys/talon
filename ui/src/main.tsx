import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../app/globals.css';
import DebuggerPage from '../app/page';
import { ThemeProvider } from '../components/theme-provider';
import { TalonQueryProvider } from '../components/query-provider';

const root = document.getElementById('root');

if (!root) {
  throw new Error('Root element #root was not found.');
}

createRoot(root).render(
  <StrictMode>
    <ThemeProvider>
      <TalonQueryProvider>
        <DebuggerPage />
      </TalonQueryProvider>
    </ThemeProvider>
  </StrictMode>,
);
