# mdviz 调试经验教训 — 通用 Debug SOP

> 基于 `D:\open_claw_agent\Beneh\GA_tools\mdviz` 项目实战总结
> 适用于 Rust HTTP 服务 + wry/tao webview + 模板渲染的调试场景

---

## 一、format!() 模板占位符检查

### 教训
`format!(r#"<div class='content'>{}</div>"#, sidebar = sidebar)` 中 `{}`（位置占位符）如果没有对应的位置参数，会意外匹配到第一个命名参数 `sidebar`，导致侧边栏内容被渲染到右栏。

### SOP
1. 一律使用 **命名占位符** `{content}` + 命名参数 `content = content`
2. 禁止混用位置占位符 `{}` 和命名参数，除非明确需要
3. 检查模板中的 `{}` 数量是否与参数数量一致

### 相关
```rust
// 错误示例
format!(r#"<div class='content'>{}</div>"#, sidebar = sidebar)
// → {} 意外匹配 sidebar，右栏显示侧边栏内容

// 正确示例
format!(r#"<div class='content'>{content}</div>"#, sidebar = sidebar, content = content)
// → {content} 正确绑定到 content 变量
```

---

## 二、TcpListener 绑定冲突

### 教训
`main()` 中两次调用 `TcpListener::bind(&addr)`（一次在主线程，一次在 `thread::spawn`），第二次绑定因端口已被占用而失败，导致进程静默崩溃。

### SOP
1. 一个端口只绑定一次 `TcpListener`
2. 需要多线程处理连接时：主线程绑定的 listener 可以通过 `mpsc` 或闭包捕获传递给后台线程
3. 使用 `set_nonblocking(true)` 的 listener 会导致 `incoming()` 返回 `WouldBlock`，需要 `std::thread::sleep` 回退
4. 如果后台线程只处理请求，不负责监听，可以将 `listener` move 到后台线程

### 相关
```rust
fn main() {
    let listener = TcpListener::bind(&addr).expect("Failed to bind");
    // 错误的做法：再spawn一个线程重新绑
    thread::spawn(move || {
        let listener2 = TcpListener::bind(&addr)... // 端口已被占用！
    });
    
    // 正确的做法：listener move到后台线程，或者用 Arc
    thread::spawn(move || {
        for stream in listener.incoming() { ... }
    });
}
```

---

## 三、set_nonblocking 对 TcpStream 的传染性

### 教训
`listener.set_nonblocking(true)` 后，通过 `listener.incoming()` 接收的 `TcpStream` 也继承非阻塞模式。在 `handle_client` 中 `stream.read()` 立即返回 `WouldBlock`，导致 HTTP 请求未被读取就关闭连接。

### SOP
1. 非阻塞 listener + 非阻塞 TcpStream = `read()` 不等待数据
2. 要么在 `handle_client` 中设为阻塞 `stream.set_nonblocking(false)`
3. 要么 listener 不设非阻塞，让 `incoming()` 阻塞等待连接
4. 选择原则：
   - 单线程处理所有连接 → `set_nonblocking(true) + epoll/select`
   - 每连接一线程 → 阻塞模式更简单（不用非阻塞）

---

## 四、wry/tao event loop 必须在主线程

### 教训
`tao::event_loop::EventLoop::run()` 必须在应用的主线程上调用。如果在 `thread::spawn` 的后台线程调用，进程立刻崩溃（无错误输出）。

### SOP
1. HTTP 服务器可以放后台线程，webview/event loop 必须放主线程
2. 结构：
   ```rust
   fn main() {
       thread::spawn(move || { /* HTTP server */ });
       open_webview(&url); // 主线程，阻塞
   }
   ```
3. `#![windows_subsystem = "windows"]` 会隐藏控制台，所有 `println!` 和 panic 输出不可见 → 调试期间暂时移除

---

## 五、Webview 缓存问题

### 教训
wry webview 启动时如果服务器尚未就绪，会加载 WebView2 的错误页面或缓存页。此后即使用 `/pdf` 端点或 `no-cache` 头也无法刷新。

### SOP
1. 在 `open_webview()` 之前加入 `std::thread::sleep(Duration::from_millis(500))` 确保 HTTP 服务器已就绪
2. HTTP 响应头添加 `Cache-Control: no-cache, no-store, must-revalidate`
3. 如果 webview 仍然显示旧内容，检查是否打开了多个实例
4. 终极方案：URL 追加随机参数 `?t=timestamp`

---

## 六、`#![windows_subsystem = "windows"]` 的使用时机

### 教训
此属性隐藏控制台窗口，但也会隐藏所有 `println!` 输出和 panic 信息。调试期间加上后会完全无法定位崩溃原因。

### SOP
1. **调试阶段**：删除或注释 `#![windows_subsystem = "windows"]`
2. **发布阶段**：再加回，同时确保有日志输出到文件
3. 如果必须保留，用 `std::fs::write` 将错误写入日志文件
   ```rust
   std::fs::write("error.log", format!("{:?}", e)).ok();
   ```

---

## 七、rustc 版本兼容性

### 教训
Rust 1.95.0 下使用 `let path = ...` 作为变量名时，编译器报 `expected value, found built-in attribute 'path'`。`path` 是 Rust 内置 `#[path]` attribute 名称，在闭包或复杂上下文中可能被误解。

### SOP
1. 避免使用 `path`、`mod`、`type`、`self` 等 Rust 关键字/内置属性名称作为变量名
2. 改用 `req_path`、`file_path`、`current_path`、`raw_path` 等
3. 遇到 `E0423` 错误时首先检查变量名是否与内置名称冲突

---

## 八、Chrome headless `--print-to-pdf` 的使用

### 教训
调用 Chrome 生成 PDF 时，输出路径必须指向源文件同目录（用户期望），而不是临时目录。Chrome 的错误输出中 `stderr` 会包含 `X bytes written to file` 的提示。

### SOP
```rust
// 正确：PDF 存到源文件同目录
let parent_dir = file.parent().and_then(|p| p.to_str()).unwrap_or(".");
let stem = file.file_stem().and_then(|x| x.to_str()).unwrap_or("output");
let tmp_pdf = std::path::PathBuf::from(parent_dir).join(format!("{}.pdf", stem));
// 而非：let tmp_pdf = std::env::temp_dir().join("output.pdf");
```

---

## 九、修改验证闭环

### 教训
多次出现"源码改了但编译后没生效"的情况，原因包括：
- 增量编译缓存（`cargo check` 通过但 `cargo build --release` 用的旧缓存）
- 启动了旧的 exe（没 kill 掉之前的进程）
- `DETACHED_PROCESS` 启动方式与 webview 不兼容

### SOP
验证修改是否生效的标准流程：
1. `taskkill /f /im mdviz.exe`
2. `cargo build --release`
3. `subprocess.Popen([exe], stdout=DEVNULL, stderr=DEVNULL)`
4. `time.sleep(3)`
5. `socket.connect(('127.0.0.1', port))` 确认端口开放
6. `http.client.HTTPConnection` 发送请求获取原始 HTML
7. 正则提取关键区域（content div、sidebar nav）验证内容正确
8. 最后用浏览器打开 URL 确认视觉效果

---

## 十、导出功能调试

### 教训
- `window.print()` 在 wry/tao webview 中只打印可见区域+触发 `CloseRequested` → 应用退出
- JS `window.open()` 在 webview 中可能被拦截
- `WebViewBuilder::build()` 在 wry 0.41 中不返回 Result，不需要 `.unwrap()`

### SOP
1. PDF 导出：使用 Chrome headless `--print-to-pdf` 在服务端生成
2. HTML 导出：克隆文档全文（`document.documentElement.outerHTML`），移除侧边栏和工具栏
3. DOC 导出：提取 `.content` 的 innerHTML 作为 HTML 保存为 `.doc`
4. 所有导出按钮触发后，通过 `<a>` 标签的 `download` 属性或 `URL.createObjectURL` 实现下载，不依赖 `window.print()`

---

## 十一、文件过滤的 `rsplit` 陷阱

### 教训
```rust
// 错误：rsplit('.').last() 返回文件名而非扩展名
// "file.md".rsplit('.').last() → "file" ❌
if supported.contains(&name.rsplit('.').last()) { ... }

// 正确：rsplit('.').next() 返回最右段（扩展名）
// "file.md".rsplit('.').next() → Some("md") ✓
if supported.contains(&name.rsplit('.').next()) { ... }
```

`rsplit` 正向迭代先返回最右边的分割片段，`next()` 得到扩展名，`last()` 得到文件名。

---

## 十二、文件链接路径格式

### 教训
侧边栏/内容区的文件链接使用**相对路径**（`report.md`）时，从 `/list/` 路径下点击后浏览器解析为 `/list/.../report.md`，被路由错误拦截为目录请求。侧边栏使用**绝对路径**（`/D:/docs/report.md`）则正常。

### SOP
文件链接一律使用绝对路径，不依赖当前 URL 上下文：
```rust
// 错误
let url = name.to_string(); // 相对路径

// 正确
let url = format!("/{}/{}", current_path, name); // 绝对路径
```

---

## 十三、交付前必须测试验证（核心铁律）

### 教训
本会话中连续出现"修复完就交付，交付后发现仍有 Bug"的循环：
- 首次修复 `/navigate` 跳转后，未测试就交付 → 用户发现 Enter 键不支持
- 隐藏黑窗后，未测试就交付 → 用户发现导出不跟随主题、HTML 文件显示源码、404 无回退
- 每次修复看似"完成"，但缺乏系统性的测试覆盖，导致反复修补

### 核心原则
**"修改完必须测试、测试通过才交付、交付前做端到端验证"** — 这条铁律不可跳过。

### SOP

#### 1. 修改后的验证清单（每次改动后自动执行）
```
[ ] cargo build --release 编译无错误
[ ] 启动应用，确认端口能连通
[ ] HTTP GET 请求关键端点验证行为
[ ] 处理 3 种情况：正常路径、边界路径、错误路径
[ ] 用户可见的 UI 元素（按钮、输入框、工具栏）的行为
```

#### 2. 本会话中暴露的常见漏洞类型

| 漏洞类型 | 典型场景 | 预防措施 |
|----------|----------|----------|
| **前端路由缺后端** | JS 跳转到 `/navigate?path=XXX` 但无对应路由 | 添加新路由后检查后端有无对应 handler |
| **Rust format! 转义** | JS 中 `{window.location.href=...}` 被 Rust 解析为占位符 | 用 `{{}}` 转义 JS 中的 `{}`；编译报错时立刻检查 |
| **路径格式错误** | `format!("{}\\", ...)` 产生 `D\` 而非 `D:\` | 验证 Windows 路径格式：`{}:\\` vs `{}\\` |
| **两处模板副本** | 目录页和文件页各一套 HTML/JS 模板，只改了一处 | 修改前 `grep` 确认有几处副本 |
| **导出忽略样式** | exportDOC 提取 innerHTML 时丢弃所有 CSS | 导出函数需手动注入当前主题的 CSS 变量 |
| **扩展名被错分类** | `.html` 被当成代码文件显示源码 | `handle_file` 中每种扩展名要单独明确处理 |
| **Webview 无回退** | 404 后用户无法返回，只能关闭应用 | 所有错误页面加 `history.back()` 回退按钮 |

#### 3. 强制闭环流程
```
修改代码 → cargo build --release → 启动应用 → HTTP 测试 → 验证结果 → 清理环境 → 交付
                                                          ↓ 失败
                                                    回到修改代码
```
每个步骤不可省略。特别是 `cargo build --release` 必须跑过（`cargo check` 不够），因为增量编译可能漏掉改动。

#### 4. 多条改动分批验证
如果同时改动了多个不相关的点（如本例中的路由 + Enter 键 + 黑窗 + 导出 + HTML 渲染 + 404 回退），每改一个就验证一个，不要攒到最后一口气测。
```
推荐：改1 → 测1 → 改2 → 测2 → 改3 → 测3
不推荐：改1+2+3+4+5 → 测1+2+3+4+5（问题叠加难以定位）
```

#### 5. 测试脚本模板
```python
# 每次修改后运行的快速测试
def quick_test(port):
    tests = [
        ("GET", "/", 200),                     # 首页
        ("GET", "/navigate", 400),             # 缺参数
        ("GET", "/navigate?path=D:\\", 302),   # 盘符跳转
        ("GET", "/nonexistent_file.xyz", 404), # 404 含回退按钮
        ("GET", "/navigate?path=nonexistent", 302),  # 不存在路径重定向
    ]
    for method, path, expected in tests:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
        conn.request(method, path)
        resp = conn.getresponse()
        body = resp.read().decode()
        assert resp.status == expected, f"{path}: expected {expected}, got {resp.status}"
        if resp.status == 404:
            assert "history.back" in body, "404 page missing back button"
        conn.close()
    print("ALL TESTS PASSED")
```
