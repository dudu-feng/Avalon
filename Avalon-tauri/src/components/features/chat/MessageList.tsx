import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { ChatMessage } from '../../../types/chat';
import { MessageBubble } from './MessageBubble';
import { StreamingBubble } from './StreamingBubble';
import { ScrollArea, Skeleton } from '../../ui';
import styles from './MessageList.module.css';

// 滚轮缓冲触发常量：到顶后继续上滑累积缓冲距离，超阈值才加载更早（防误加载 + 主动触发）
const THRESHOLD = 48; // 触发加载的最小缓冲距离（px）
const MAX_PULL = 64; // 缓冲距离上限（阻尼衰减后趋近）
const MIN_HEIGHT = 32; // 提示条最小高度（armed 态）
const LOADING_HEIGHT = 36; // 加载态高度（容纳转圈）
const DAMPING = 0.5; // 滚轮缓冲阻尼系数（越拉越慢）
const SETTLE_MS = 120; // 停止滚轮后判定阈值 / 回弹的延迟

export interface MessageListProps {
  messages: ChatMessage[];
  /** 加载中（初始拉取历史 / 新会话处理中）：显示骨架屏占位 */
  loading?: boolean;
  /** 会话切换标识（activeId）：变化时重置滚动定位，切到新会话后从底部开始 */
  resetKey?: string;
  /** 是否还有更早块（决定滚动到顶是否显示加载入口） */
  hasEarlier?: boolean;
  /** 是否正在加载更早历史（驱动转圈态） */
  loadingEarlier?: boolean;
  /** 主动加载更早一块 */
  onLoadEarlier?: () => void;
  /** 是否在列表末尾渲染实时流式气泡（只有主对话页需要；任务历史等静态视图不订阅高频状态） */
  withStreaming?: boolean;
  /** 当前来源筛选是否包含流式消息 */
  streamingVisible?: boolean;
}

export function MessageList({
  messages,
  loading = false,
  resetKey,
  hasEarlier = false,
  loadingEarlier = false,
  onLoadEarlier,
  withStreaming = false,
  streamingVisible = true,
}: MessageListProps) {
  const endRef = useRef<HTMLDivElement>(null);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const initializedRef = useRef(false);

  // 渐进式加载：滚轮缓冲状态（mode 状态机 + 缓冲距离）
  const [pullDistance, setPullDistance] = useState(0);
  const [mode, setMode] = useState<'idle' | 'armed' | 'pulling' | 'loading'>('idle');
  // 事件回调里读最新值的 ref（避免闭包陷阱 / effect 频繁重绑）
  const modeRef = useRef(mode);
  const pullRef = useRef(pullDistance);
  const settleTimerRef = useRef<number | undefined>(undefined);
  const wasLoadingRef = useRef(false);
  const prevScrollHeightRef = useRef(0);

  // ScrollArea 透传的 viewport ref
  const setViewportRef = useCallback((el: HTMLDivElement | null) => {
    viewportRef.current = el;
  }, []);

  // 是否接近底部（用于对话更新时决定是否跟随滚动，避免打断用户上滚阅读）
  const nearBottom = useCallback(() => {
    const vp = viewportRef.current;
    if (!vp) return true;
    return vp.scrollHeight - vp.scrollTop - vp.clientHeight < 120;
  }, []);

  // 内容是否真的超出视口：内容短于视口时 scrollTop 恒为 0，
  // 不能把「在顶部」当成可加载信号，否则 armed 提示条会永久停留。
  const canScroll = useCallback(() => {
    const vp = viewportRef.current;
    return !!vp && vp.scrollHeight > vp.clientHeight + 1;
  }, []);

  // 流式气泡的存在性（每轮才变一次）；内容增长不走列表 state，避免每次增量重渲染整列
  const [streamPresent, setStreamPresent] = useState(false);
  const handleStreamPresence = useCallback((present: boolean) => setStreamPresent(present), []);

  // 流式内容增长：流式气泡自己触发，而不是让列表订阅高频状态
  const handleStreamGrow = useCallback(() => {
    if (!initializedRef.current) return;
    if (nearBottom()) {
      endRef.current?.scrollIntoView({ behavior: 'auto', block: 'end' });
    }
  }, [nearBottom]);

  // 复位滚轮缓冲状态（ref + state 同步改，避免事件回调读到旧值）
  const resetPull = useCallback(() => {
    window.clearTimeout(settleTimerRef.current);
    pullRef.current = 0;
    modeRef.current = 'idle';
    setPullDistance(0);
    setMode('idle');
  }, []);

  // 会话切换（resetKey 变化）：重置初始化标志，令下方 useLayoutEffect 重新 instant 定位到底；
  // 同时撤掉上一会话可能残留的「加载更早」提示条。
  useLayoutEffect(() => {
    if (resetKey !== undefined) initializedRef.current = false;
    resetPull();
  }, [resetKey, resetPull]);

  // 没有更早内容时，任何残留的 armed / loading 提示条都应立即消失
  useEffect(() => {
    if (!hasEarlier) resetPull();
  }, [hasEarlier, resetPull]);

  // 首次加载历史：在绘制前 instant 定位到底，消除「先渲染顶部再滚到底」的闪烁与慢滚动
  useLayoutEffect(() => {
    if (initializedRef.current || (messages.length === 0 && !streamPresent)) return;
    endRef.current?.scrollIntoView({ block: 'end' });
    initializedRef.current = true;
  }, [messages, streamPresent]);

  // 提交消息更新：仅当用户接近底部时才跟随（上滚阅读历史时保持原位）。
  // 流式增量不走这里（由 StreamingBubble 的 onGrow 处理），用 auto 避免动画重启。
  useEffect(() => {
    if (!initializedRef.current) return;
    if (nearBottom()) {
      endRef.current?.scrollIntoView({ behavior: streamPresent ? 'auto' : 'smooth', block: 'end' });
    }
  }, [messages, streamPresent, nearBottom]);

  // 滚轮缓冲触发：到顶后继续上滑累积缓冲，超阈值主动加载更早一块
  useEffect(() => {
    const vp = viewportRef.current;
    if (!vp) return;

    const atTop = () => vp.scrollTop <= 0;
    const setPull = (d: number, m: 'idle' | 'armed' | 'pulling' | 'loading') => {
      pullRef.current = d;
      modeRef.current = m;
      setPullDistance(d);
      setMode(m);
    };

    // 停止滚轮后判定：达阈值触发加载，否则回弹
    const settle = () => {
      if (modeRef.current === 'loading') return;
      if (pullRef.current >= THRESHOLD) {
        setPull(LOADING_HEIGHT, 'loading');
        onLoadEarlier?.();
      } else {
        // 视口本来就装得下内容时，回弹后直接隐藏：没有「停在顶部」的滚动意义
        setPull(0, canScroll() ? 'armed' : 'idle');
      }
    };

    const onWheel = (e: WheelEvent) => {
      if (!hasEarlier || loadingEarlier) return;
      if (!atTop()) return;

      if (e.deltaY < 0) {
        // 继续上滑：累积缓冲（阻尼系数让「越拉越慢」）
        const next = Math.min(pullRef.current + -e.deltaY * DAMPING, MAX_PULL);
        setPull(next, 'pulling');
        window.clearTimeout(settleTimerRef.current);
        settleTimerRef.current = window.setTimeout(settle, SETTLE_MS);
      } else if (e.deltaY > 0 && modeRef.current === 'pulling') {
        // 向下滚回：回弹，不加载
        window.clearTimeout(settleTimerRef.current);
        setPull(0, 'armed');
      }
    };

    const onScroll = () => {
      if (atTop()) {
        if (modeRef.current === 'loading') return;
        if (modeRef.current === 'pulling') return; // 上滑过程中不打断
        if (hasEarlier && !loadingEarlier && canScroll()) {
          setPull(0, 'armed');
        } else if (modeRef.current === 'armed') {
          // 已无可加载内容 / 内容装得下视口：撤掉停在顶部的提示条
          setPull(0, 'idle');
        }
      } else if (modeRef.current !== 'idle') {
        // 离开顶部：回 idle，缓冲清零
        window.clearTimeout(settleTimerRef.current);
        setPull(0, 'idle');
      }
    };

    vp.addEventListener('wheel', onWheel, { passive: true });
    vp.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      vp.removeEventListener('wheel', onWheel);
      vp.removeEventListener('scroll', onScroll);
      window.clearTimeout(settleTimerRef.current);
    };
  }, [hasEarlier, loadingEarlier, onLoadEarlier]);

  // 加载更早完成：恢复滚动位置（头部插入更早消息后保持视口稳定）+ 复位加载态
  useLayoutEffect(() => {
    const vp = viewportRef.current;
    const wasLoading = wasLoadingRef.current;
    wasLoadingRef.current = loadingEarlier;

    if (!vp) return;

    if (!wasLoading && loadingEarlier) {
      // 开始加载：记录旧 scrollHeight
      prevScrollHeightRef.current = vp.scrollHeight;
    } else if (wasLoading && !loadingEarlier) {
      // 加载完成：把 scrollTop 加上新增高度，视口保持在原消息位置
      vp.scrollTop += vp.scrollHeight - prevScrollHeightRef.current;
      window.clearTimeout(settleTimerRef.current);
      pullRef.current = 0;
      setPullDistance(0);
      // 仍停在顶部且还能滚动 / 还有更早块时才回到 armed，否则彻底隐藏
      const stillArmed =
        hasEarlier && vp.scrollTop <= 0 && vp.scrollHeight > vp.clientHeight + 1;
      modeRef.current = stillArmed ? 'armed' : 'idle';
      setMode(modeRef.current);
    }
  }, [loadingEarlier, messages, hasEarlier]);

  if (loading) {
    return (
      <div className={styles.skeletonList} aria-hidden="true">
        <div className={styles.skeletonRowRight}>
          <Skeleton className={styles.skeletonUser} />
        </div>
        <div className={styles.skeletonRowLeft}>
          <Skeleton className={styles.skeletonAssistantLg} />
        </div>
        <div className={styles.skeletonRowRight}>
          <Skeleton className={styles.skeletonUserSm} />
        </div>
        <div className={styles.skeletonRowLeft}>
          <Skeleton className={styles.skeletonAssistantMd} />
        </div>
      </div>
    );
  }

  if (messages.length === 0 && !streamPresent) {
    return (
      <div className={styles.empty}>
        <p className={styles.emptyTitle}>开始和 Avalon 对话</p>
        <p className={styles.emptyHint}>输入消息，与你的智能助手交流。</p>
      </div>
    );
  }

  return (
    <div className={styles.wrap}>
      <ScrollArea
        viewportRef={setViewportRef}
        className={styles.listRoot}
        viewportClassName={styles.list}
      >
        {messages.map((m) => (
          <MessageBubble key={m.id} message={m} />
        ))}
        {withStreaming && (
          <StreamingBubble
            visible={streamingVisible}
            onGrow={handleStreamGrow}
            onPresenceChange={handleStreamPresence}
          />
        )}
        <div ref={endRef} />
      </ScrollArea>

      {mode !== 'idle' && (
        <div
          className={styles.pullIndicator}
          style={{
            height: mode === 'loading' ? LOADING_HEIGHT : Math.max(pullDistance, MIN_HEIGHT),
          }}
        >
          {loadingEarlier ? (
            <span className={styles.spinner} />
          ) : (
            <span className={styles.pullLabel}>⟳ 加载更早</span>
          )}
        </div>
      )}
    </div>
  );
}
