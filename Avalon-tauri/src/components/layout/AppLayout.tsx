// ============================================================
// AppLayout —— 应用主布局
//
// 使用 antd 的 Layout 组件搭建整体框架：
// - 左侧 Sidebar（会话列表、设置入口）
// - 右侧 Content（聊天窗口）
//
// 【React 概念】Props 解构
//   在 Vue 中: defineProps<{ children: ReactNode }>()
//   在 React 中: 直接在函数参数中解构 { children }
//   children 是 React 的特殊 prop，表示组件标签内的内容。
// ============================================================

import React, { useState } from "react";
import { Layout, Menu, Button, theme } from "antd";
import {
  MessageOutlined,
  SettingOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import type { MenuProps } from "antd";

const { Header, Sider, Content } = Layout;

// ============================================================
//  Props 类型定义
//
//  React 中定义 Props 的两种方式：
//  1. interface Props { ... }  —— 推荐，可继承
//  2. type Props = { ... }      —— 更灵活，支持联合类型
//
//  对比 Vue: defineProps<{ ... }>() 或 Props 泛型
// ============================================================
interface AppLayoutProps {
  /** 子组件（内容区域），类似 Vue 的 <slot /> */
  children: React.ReactNode;
}

/**
 * 主布局组件
 *
 * React 函数组件 = 一个返回 JSX 的普通函数。
 * 对比 Vue: <script setup> 中定义的组件。
 */
const AppLayout: React.FC<AppLayoutProps> = ({ children }) => {
  // ==========================================================
  //  本地状态 —— 控制侧边栏折叠/展开
  //
  //  React 中，useState 返回 [值, setter]。
  //  不像 Vue 的 ref() 可以直接 .value 修改，
  //  React 必须通过 setter 函数来更新状态。
  // ==========================================================
  const [collapsed, setCollapsed] = useState(false);
  const [currentMenu, setCurrentMenu] = useState("chat");

  // antd 6 的 token 系统（获取主题色）
  const { token } = theme.useToken();

  /**
   * 侧边栏菜单配置
   *
   * 注意：这里直接用数组定义，没有用 useMemo 包裹。
   * 因为这个数组不依赖任何 props/state，不需要缓存。
   */
  const menuItems: MenuProps["items"] = [
    {
      key: "chat",
      icon: <MessageOutlined />,
      label: "对话",
    },
    {
      key: "settings",
      icon: <SettingOutlined />,
      label: "设置",
    },
  ];

  /** 菜单点击处理 —— 普通函数即可，不需要 useCallback（没有传给深层子组件） */
  const handleMenuClick: MenuProps["onClick"] = (e) => {
    setCurrentMenu(e.key);
  };

  return (
    /**
     * JSX 语法说明：
     * - className 替代 HTML 的 class（因为 class 是 JS 关键字）
     * - style 接受一个对象，属性名用 camelCase：{ backgroundColor: "red" }
     *   对比 Vue: :style="{ backgroundColor: 'red' }"
     * - 花括号 {} 内写 JS 表达式，对比 Vue template 中的 {{ }}
     */
    <Layout style={{ height: "100vh" }}>
      {/* ======================================================
        Sider —— 侧边栏
        collapsed 控制折叠状态，类似 Vue 的 :collapsed="collapsed"
      ====================================================== */}
      <Sider
        trigger={null}
        collapsible
        collapsed={collapsed}
        width={220}
        style={{
          background: token.colorBgContainer,
          borderRight: `1px solid ${token.colorBorderSecondary}`,
        }}
      >
        {/* 新建对话按钮 */}
        <div
          style={{
            padding: collapsed ? "12px 8px" : "16px",
            textAlign: "center",
          }}
        >
          <Button
            type="primary"
            icon={<PlusOutlined />}
            block={!collapsed}
            shape={collapsed ? "circle" : "default"}
          >
            {/* React 条件渲染：用 {} 包裹 JS 表达式 */}
            {!collapsed && "新建对话"}
          </Button>
        </div>

        {/* 菜单 */}
        <Menu
          mode="inline"
          selectedKeys={[currentMenu]}
          items={menuItems}
          onClick={handleMenuClick}
          style={{ borderInlineEnd: "none" }}
        />
      </Sider>

      {/* ======================================================
        Layout —— 右侧主区域
      ====================================================== */}
      <Layout>
        {/* Header —— 顶部栏 */}
        <Header
          style={{
            padding: "0 16px",
            background: token.colorBgContainer,
            display: "flex",
            alignItems: "center",
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
            height: 48,
          }}
        >
          {/* 折叠按钮 */}
          <Button
            type="text"
            icon={collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
            onClick={() => setCollapsed(!collapsed)}
            //     ↑ onClick 对比 Vue: @click="collapsed = !collapsed"
            //     注意 React 的 setState 写法 vs Vue 的直接赋值
          />
          <span style={{ marginLeft: 12, fontWeight: 500, fontSize: 16 }}>
            Avalon
          </span>
        </Header>

        {/* Content —— 内容区，渲染子组件 */}
        <Content
          style={{
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
            background: token.colorBgLayout,
          }}
        >
          {/* children 就是插槽内容，对比 Vue 的 <slot /> */}
          {children}
        </Content>
      </Layout>
    </Layout>
  );
};

export default AppLayout;
