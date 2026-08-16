import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

import { MainLayout, Sidebar, Header } from './components/layout';
import { Button, Input, Card, Badge } from './components/ui';
import { ThemeToggle } from './components/ui/ThemeToggle';
import { ChatIcon, HomeIcon, LibraryIcon, SettingsIcon } from './components/icons';
import { useTheme } from './hooks/useTheme';
import type { MenuItemData } from './types';
import styles from './App.module.css';

const menuItems: MenuItemData[] = [
  { id: 'home', label: 'Home', icon: <HomeIcon /> },
  { id: 'chat', label: 'Chat', icon: <ChatIcon /> },
  { id: 'library', label: 'Library', icon: <LibraryIcon /> },
  { id: 'settings', label: 'Settings', icon: <SettingsIcon /> },
];

function App() {
  const { mode, setMode } = useTheme();
  const [activeId, setActiveId] = useState('home');
  const [name, setName] = useState('');
  const [greetMsg, setGreetMsg] = useState('');
  const [isLoading, setIsLoading] = useState(false);

  async function greet() {
    if (!name.trim()) return;
    setIsLoading(true);
    try {
      const message = await invoke<string>('greet', { name });
      setGreetMsg(message);
    } finally {
      setIsLoading(false);
    }
  }

  const sidebar = (
    <Sidebar
      title="Avalon"
      items={menuItems}
      activeId={activeId}
      onSelect={setActiveId}
    />
  );

  const header = (
    <Header
      title={menuItems.find((item) => item.id === activeId)?.label ?? 'Avalon'}
      actions={<ThemeToggle mode={mode} onChange={setMode} />}
    />
  );

  return (
    <MainLayout sidebar={sidebar} header={header}>
      <div className={styles.grid}>
        <section className={styles.section}>
          <h2 className={styles.heading}>Welcome back</h2>
          <p className={styles.lead}>
            A calm, modular Tauri interface built with React and TypeScript.
          </p>

          <div className={styles.row}>
            <Badge variant="filled">React 19</Badge>
            <Badge variant="muted">Tauri 2</Badge>
            <Badge variant="outline">TypeScript</Badge>
          </div>
        </section>

        <section className={styles.section}>
          <Card
            eyebrow="Tauri command"
            title="Greet from Rust"
            description="Enter your name and call the Rust backend through a typed invoke channel."
          >
            <div className={styles.form}>
              <Input
                label="Your name"
                value={name}
                placeholder="Enter a name..."
                onChange={(e) => setName(e.currentTarget.value)}
                onKeyDown={(e) => e.key === 'Enter' && greet()}
              />
              <Button
                variant="primary"
                onClick={greet}
                disabled={isLoading || !name.trim()}
              >
                {isLoading ? 'Greeting…' : 'Greet'}
              </Button>
            </div>
            {greetMsg && <p className={styles.greetMsg}>{greetMsg}</p>}
          </Card>
        </section>

        <section className={styles.cards}>
          <Card
            eyebrow="Design"
            title="Claude-inspired tokens"
            description="Warm neutrals, a single terra-cotta accent, and editorial typography keep the UI calm."
            variant="sunken"
          />
          <Card
            eyebrow="Architecture"
            title="Modular components"
            description="Each piece — Sidebar, MenuItem, Button, Card — is self-contained and easy to extend."
          />
        </section>
      </div>
    </MainLayout>
  );
}

export default App;
