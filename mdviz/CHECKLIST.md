# mdviz 交付标准检查清单

> 基于 `D:\open_claw_agent\Beneh\GA_tools\mdviz` 项目
> 每次修改后必须逐项检查，测试通过后方可交付

---

## A. 编译与二进制

- [ ] `cargo build --release` 编译无错误（仅允许 warning）
- [ ] PE subsystem = 2（Windows GUI，无控制台黑窗）
- [ ] exe 文件存在且可执行

## B. 服务启动

- [ ] 启动后能监听到端口（默认 20000）
- [ ] HTTP 响应正常（不超时、不崩溃）

## C. 路由 — 目录/列表

| 路由 | 期望状态 | 说明 |
|------|----------|------|
| `GET /` | 200 | 首页，显示空目录+侧边栏 |
| `GET /list/` | 200 | 同首页 |
| `GET /list/D:/` | 200 | 显示 D 盘根目录列表 |
| `GET /list/{有效路径}/` | 200 | 显示指定目录内容 |

## D. 路由 — 路径跳转 (`/navigate`)

| 路由 | 期望状态 | 说明 |
|------|----------|------|
| `GET /navigate` | 400 | 缺参数 |
| `GET /navigate?path=D:\` | 302 → `/list/D:/` | 盘符 |
| `GET /navigate?path={有效目录}` | 302 → `/list/{path}/` | 目录 |
| `GET /navigate?path={有效文件}` | 302 → `/{path}` | 文件 |
| `GET /navigate?path={不存在路径}` | 302 到父目录 或 404+回退按钮 | 不存在 |

## E. 路由 — 文件预览

| 路由 | 期望状态 | 说明 |
|------|----------|------|
| `GET /{有效.md文件}` | 200 | Markdown 渲染为 HTML |
| `GET /{有效.html文件}` | 200 | 直接返回 raw HTML（浏览器渲染） |
| `GET /{有效.代码文件}` | 200 | 代码文件显示为 `<pre><code>` |
| `GET /{有效.文本文件}` | 200 | txt/log 显示为文本 |
| `GET /{不支持的文件}` | 403 | 如 .exe .png |
| `GET /{不存在的文件}` | 404 + `history.back()` 按钮 | |

## F. 其他路由

| 路由 | 期望状态 | 说明 |
|------|----------|------|
| `GET /favicon.ico` | 404 | 无图标，不给 500 |
| `GET /pdf/{有效文件}` | 200/302 | PDF 导出端点 |

## G. 用户界面元素（HTML 中存在）

- [ ] 导出按钮：exportPDF、exportHTML、exportDOC
- [ ] 路径输入框 `id='navPath'`，带 `onkeydown` Enter 监听
- [ ] 跳转按钮
- [ ] 主题切换按钮（4 种：light/dark/sepia/sepiadark）
- [ ] 侧边栏（目录导航）

## H. 主题系统

- [ ] 全局 `themes` 对象含 4 个变体（light, dark, sepia, sepiadark）
- [ ] `exportDOC()` 引用 `themes[t]`（全局变量，非局部）
- [ ] `applyTheme()` 引用 `themes[t]`（全局变量，非局部）
- [ ] 主题切换按钮调用 `localStorage.setItem('theme', ...)` + `applyTheme()`

## I. 404 页面

- [ ] 所有 404 页面包含 `<a href='javascript:history.back()'>` 回退按钮
- [ ] 文件不存在的 404
- [ ] 路径不存在的 404（navigate handler）
- [ ] PDF 不存在的 404

## J. 边界情况

- [ ] 路径含空格（如 `C:\Program Files`）
- [ ] 路径含中文
- [ ] 路径含特殊字符（如 `+`, `&`, `#`）
- [ ] 磁盘根目录（`D:\`）
- [ ] 极大目录
- [ ] 空路径
- [ ] 混合分隔符（`D:/path\to\file`）
