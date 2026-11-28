import { defineConfig } from "vitest/config";

// 前端单测独立于 Tauri/Vite 开发服务器配置：这里只跑纯函数与 store 归约，
// 不启动 dev server，也不需要 React DOM 环境。
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
