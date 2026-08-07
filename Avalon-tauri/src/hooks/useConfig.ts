// ============================================================
// useConfig —— 应用配置管理 Hook
//
// 管理配置的加载、保存和校验。
// ============================================================

import { useState, useCallback, useEffect } from "react";
import type { AppConfig } from "../types";
import { getConfig, saveConfig, validateConfig, initApp } from "../services/tauriApi";

export function useConfig() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  // ==========================================================
  //  useEffect —— React 的"副作用"处理
  //
  //  useEffect(setup, [依赖项])
  //  - setup 函数在组件挂载后执行
  //  - 依赖项变化时重新执行
  //  - 返回的 cleanup 函数在组件卸载/重新执行前调用
  //
  //  对比 Vue 3:
  //    onMounted(() => { ... })              // 挂载后
  //    watch([dep1, dep2], () => { ... })    // 依赖变化
  //    onUnmounted(() => { ... })            // 卸载前
  //
  //  ⚠️ 依赖项为空数组 [] = 只在挂载时执行一次 ≈ onMounted
  //  ⚠️ 不传依赖项 = 每次渲染都执行（通常不推荐）
  // ==========================================================
  useEffect(() => {
    loadConfig();
    // 注意：这里不需要 return cleanup 函数，因为没有订阅需要取消
  }, []); // 空数组 = 只在组件首次渲染后执行一次

  /** 加载配置并进行初始化校验 */
  const loadConfig = useCallback(async () => {
    setLoading(true);
    try {
      // 初始化应用（后端返回警告列表）
      const initWarnings = await initApp();
      // 加载完整配置
      const cfg = await getConfig();
      setConfig(cfg);
      setWarnings(initWarnings);
    } catch (err) {
      console.error("加载配置失败:", err);
      setWarnings([`加载配置失败: ${err}`]);
    } finally {
      setLoading(false);
    }
  }, []);

  /** 保存配置到后端 */
  const save = useCallback(async (newConfig: AppConfig) => {
    try {
      await saveConfig(newConfig);
      setConfig(newConfig);
      // 保存后重新校验
      const newWarnings = await validateConfig();
      setWarnings(newWarnings);
      return true;
    } catch (err) {
      console.error("保存配置失败:", err);
      return false;
    }
  }, []);

  return {
    config,
    warnings,
    loading,
    save,
    reload: loadConfig,
  } as const;
}
