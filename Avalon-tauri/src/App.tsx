import { useEffect, useState } from 'react';
import { MainLayout, Sidebar, Header } from './components/layout';
import { ThemeToggle } from './components/ui/ThemeToggle';
import { useTheme } from './hooks/useTheme';
import { pages } from './pages';
import { DEFAULT_CHANNEL, initSession } from './lib/chatApi';

function App() {
  const { mode, setMode } = useTheme();
  const [activeId, setActiveId] = useState('home');

  const active = pages.find((page) => page.id === activeId) ?? pages[0];
  const ActivePage = active.component;

  // 应用启动时初始化会话（复用 active / 否则新建），会话为应用级资源，启动即就绪
  useEffect(() => {
    initSession(DEFAULT_CHANNEL).catch((e) => console.error('init_session 失败:', e));
  }, []);

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
