// ============================================================
// App —— 应用根组件
//
// React 根组件的职责：
// 1. 提供全局配置（antd ConfigProvider）
// 2. 组装布局和页面
// 3. 初始化应用状态
//
// 对比 Vue 的 App.vue：
//   - React 没有 <template>/<script>/<style> 分离，都在 .tsx 中
//   - 样式通过 import "./App.css" 引入（或 CSS Modules）
//   - 组件直接用函数定义，不需要 export default { ... }
// ============================================================

import { useEffect } from "react";
import { ConfigProvider, App as AntdApp, theme } from "antd";
import zhCN from "antd/locale/zh_CN";
import AppLayout from "./components/layout/AppLayout";
import ChatWindow from "./components/chat/ChatWindow";
import { useConfig } from "./hooks/useConfig";
import "./App.css";

/**
 * App 组件
 *
 * React.FC 是 FunctionComponent 的缩写，包含 children 等默认类型。
 * 不过现代 React 项目倾向于直接写普通函数，不用 React.FC。
 */
function App() {
  // 加载应用配置（初始化时调用后端 init_app）
  const { warnings, loading } = useConfig();

  /**
   * useEffect —— 应用启动时的副作用
   *
   * 依赖数组为空 []：只在组件首次渲染后执行一次。
   * 对应 Vue 的 onMounted()。
   *
   * 这里可以放各种初始化逻辑：
   * - 检查配置
   * - 加载历史记录
   * - 建立连接等
   */
  useEffect(() => {
    console.log("[Avalon] 应用已启动");
    if (warnings.length > 0) {
      console.warn("[Avalon] 配置警告:", warnings);
    }
  }, []); // ← 空数组表示：永远只执行一次

  // 显示加载状态
  if (loading) {
    return (
      <div
        style={{
          height: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexDirection: "column",
          gap: 16,
        }}
      >
        <div className="app-loading-spinner" />
        <span style={{ color: "#999" }}>正在初始化...</span>
      </div>
    );
  }

  return (
    /**
     * ConfigProvider —— antd 的全局配置注入
     *
     * 类似 Vue 中在 app.use() 时传入的全局配置。
     * locale={zhCN} 设置组件文案为中文。
     *
     * AntdApp 是 antd 6 的包裹组件，提供 message/notification 等静态方法。
     */
    <ConfigProvider
      locale={zhCN}
      theme={{
        // antd 6 默认就是 css-in-js 主题系统
        algorithm: theme.defaultAlgorithm, // 浅色主题
        token: {
          colorPrimary: "#6366f1", // 主色调（可以改成你喜欢的颜色）
          borderRadius: 8,
        },
      }}
    >
      <AntdApp>
        {/* AppLayout 提供侧边栏 + 顶栏框架 */}
        <AppLayout>
          {/* ChatWindow 是实际的内容（类似 Vue Router 的 <router-view />） */}
          <ChatWindow />
        </AppLayout>
      </AntdApp>
    </ConfigProvider>
  );
}

export default App;
