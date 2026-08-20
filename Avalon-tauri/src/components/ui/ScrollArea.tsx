// 滚动区域原子组件：隐藏原生滚动条，自绘纤细 thumb，仅在用户主动滚动时显示。
//
// 「滚动时才显示」无法用 ::-webkit-scrollbar 纯 CSS 实现（它监听不到滚动状态），
// 故用 JS 监听 scroll / resize / 内容变化驱动 thumb 几何，wheel / touch 驱动显隐。

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type HTMLAttributes,
  type ReactNode,
} from 'react';
import styles from './ScrollArea.module.css';

export interface ScrollAreaProps extends HTMLAttributes<HTMLElement> {
  /** root 标签（默认 div；迁移 main/aside 时保留语义） */
  as?: 'div' | 'main' | 'aside';
  /** viewport 内容区样式（padding / gap / flex 列布局等） */
  viewportClassName?: string;
  children: ReactNode;
}

export function ScrollArea({
  as: Component = 'div',
  viewportClassName = '',
  className = '',
  children,
  ...rest
}: ScrollAreaProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [thumb, setThumb] = useState({ height: 0, top: 0 });
  const [scrollable, setScrollable] = useState(false);
  const [visible, setVisible] = useState(false);
  const hideTimer = useRef<number | undefined>(undefined);

  // 依据 viewport 的 scrollHeight / clientHeight / scrollTop 重算 thumb 高度与位置
  const updateThumb = useCallback(() => {
    const vp = viewportRef.current;
    if (!vp) return;
    const scrollableH = vp.scrollHeight - vp.clientHeight;
    if (scrollableH <= 0) {
      setScrollable(false);
      return;
    }
    setScrollable(true);
    const trackH = vp.clientHeight;
    const height = Math.max((vp.clientHeight / vp.scrollHeight) * trackH, 24);
    const top = (vp.scrollTop / scrollableH) * (trackH - height);
    setThumb({ height, top });
  }, []);

  // 用户主动滚动（滚轮/触控）：立即显示，停止 ~600ms 后淡出
  const showFromUserScroll = useCallback(() => {
    setVisible(true);
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => setVisible(false), 600);
  }, []);

  useEffect(() => {
    const vp = viewportRef.current;
    if (!vp) return;

    let raf = 0;
    // rAF 节流：scroll / resize / 内容变化合并到每帧一次，避免流式打字时高频 setState
    const scheduleUpdate = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(updateThumb);
    };

    updateThumb();

    // scroll 仅同步 thumb 几何；显隐由 wheel / touch（用户主动）驱动，
    // 因此内容增长引起的 scrollIntoView 自动滚动不会让滚动条出现。
    const onScroll = () => scheduleUpdate();

    vp.addEventListener('scroll', onScroll, { passive: true });
    vp.addEventListener('wheel', showFromUserScroll, { passive: true });
    vp.addEventListener('touchmove', showFromUserScroll, { passive: true });
    const ro = new ResizeObserver(scheduleUpdate);
    ro.observe(vp);
    const mo = new MutationObserver(scheduleUpdate);
    mo.observe(vp, { childList: true, subtree: true, characterData: true });

    return () => {
      vp.removeEventListener('scroll', onScroll);
      vp.removeEventListener('wheel', showFromUserScroll);
      vp.removeEventListener('touchmove', showFromUserScroll);
      ro.disconnect();
      mo.disconnect();
      cancelAnimationFrame(raf);
      window.clearTimeout(hideTimer.current);
    };
  }, [updateThumb, showFromUserScroll]);

  return (
    <Component className={[styles.root, className].filter(Boolean).join(' ')} {...rest}>
      <div
        ref={viewportRef}
        className={[styles.viewport, viewportClassName].filter(Boolean).join(' ')}
      >
        {children}
      </div>
      <div className={[styles.track, visible && styles.trackVisible].filter(Boolean).join(' ')}>
        {scrollable && (
          <div
            className={styles.thumb}
            style={{ height: thumb.height, transform: `translateY(${thumb.top}px)` }}
          />
        )}
      </div>
    </Component>
  );
}
