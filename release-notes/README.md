# 发布说明归档

本目录保存每个版本的 Release Notes，安装包统一通过 GitHub Release 管理，不再在仓库里跟踪 `exe_list/`。

## 版本与安装包

| 版本 | Tag | 安装包 | 说明 |
|---|---|---|---|
| 0.2.0 | `v0.2.0` | `Avalon_0.2.0_x64-setup.exe` | 首个 Tauri 桌面安装包 |
| 0.3.0 | `v0.3.0` | `Avalon_0.3.0_x64-setup.exe` | 飞书渠道、仪表盘 |
| 0.4.0 | `v0.4.0` | `Avalon_0.4.0_x64-setup.exe` | 联网搜索、飞书通知、托盘 |
| 0.4.2 | `v0.4.2` | `Avalon_0.4.2_x64-setup.exe` | 工具沙箱 |
| 0.4.3 | `v0.4.3` | `Avalon_0.4.3_x64-setup.exe` | 内置 embedding 模型 |
| 0.4.4 | `v0.4.4` | `Avalon_0.4.4_x64-setup.exe` | 超时与向量目录修复 |
| 0.4.5 | `v0.4.5` | `Avalon_0.4.5_x64-setup.exe` | 会话压缩 |
| 0.5.0 | `v0.5.0` | `Avalon_0.5.0_x64-setup.exe` | 灵魂、技能、定时任务编辑 |
| 0.6.0 | 需补 | `Avalon_0.6.0_x64-setup.exe` | 流式输出切页丢失修复 |
| 0.7.0 | `v0.7.0` | `Avalon_0.7.0_x64-setup.exe` | 文件存储、视觉、OCR |
| 0.7.2 | 待创建 | `Avalon_0.7.2_x64-setup.exe` | QQ 渠道、通讯录主人区 |

## 补齐 0.6.0 tag

仓库此前有 0.6.0 安装包和版本号提交，但没有 tag。Release 必须挂在 tag 上：

```bash
git tag -a v0.6.0 ebfe29e -m "Avalon 0.6.0"
git push origin v0.6.0
```

## 补建 Release（网页）

1. 打开 <https://github.com/dudu-feng/Avalon/releases/new>
2. 选择对应 tag，标题填 `Avalon <版本>`，正文粘贴本目录对应 `.md` 文件
3. 从本地 `exe_list/` 上传同版本安装包
4. 计算并附带校验和（可选但推荐）：

```bash
certutil -hashfile Avalon_0.5.0_x64-setup.exe SHA256
```

5. 先存 Draft 检查，再 Publish

## 补建 Release（命令行）

本机未安装 `gh`。安装后可以一次完成创建与上传：

```bash
gh release create v0.5.0 \
  "exe_list/Avalon_0.5.0_x64-setup.exe" \
  --title "Avalon 0.5.0" \
  --notes-file release-notes/v0.5.0.md
```

旧版本不要标 `--latest`，GitHub 会把最新发布的非预发布版本自动设为 Latest；补历史版本时可在网页上确认 Latest 仍指向最新版。

## 安装包管理约定

- 安装包只上传到对应 tag 的 Release，不再提交到仓库
- 附件命名保持 `Avalon_<版本>_<架构>-setup.exe`
- Release 正文以本目录文件为准，变更后再同步更新
