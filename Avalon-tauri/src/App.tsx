import { useState } from 'react';
import { MainLayout, Sidebar, Header } from './components/layout';
import { ThemeToggle } from './components/ui/ThemeToggle';
import { useTheme } from './hooks/useTheme';
import { pages } from './pages';

function App() {
  const { mode, setMode } = useTheme();
  const [activeId, setActiveId] = useState('home');

  const active = pages.find((page) => page.id === activeId) ?? pages[0];
  const ActivePage = active.component;

  const sidebar = (
    <Sidebar
      title="Avalon"
      items={pages}
      activeId={activeId}
      onSelect={setActiveId}
    />
  );

  const header = (
    <Header
      title={active.label}
      actions={<ThemeToggle mode={mode} onChange={setMode} />}
    />
  );

  return (
    <MainLayout sidebar={sidebar} header={header}>
      <ActivePage />
    </MainLayout>
  );
}

export default App;
