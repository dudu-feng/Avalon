import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { PageContainer, Button, Input, Card, Badge } from '../../components/ui';
import styles from './HomePage.module.css';

export function HomePage() {
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

  return (
    <PageContainer>
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
    </PageContainer>
  );
}
