# GUI

`packages/eorzea-app` 是正式 **Iced 0.14 + tiny-skia 软件渲染**前端。二进制名为
`eorzea-app`；CLI 仍是默认 workspace member，两者共用 `~/.eorzea/` 下的账号与配置。
不依赖框架比较目录或演示 fixtures。

## 渲染与构建

Iced 关闭默认 features，明确启用 `tiny-skia`，没有 wgpu/glow 回退，也不启用 `sysinfo`
系统清查诊断。Linux 使用 winit 的 Wayland/X11 与 softbuffer；GUI 本身不需要 Vulkan。
游戏经 Wine/DXVK 运行的图形依赖独立保留。中文字体来自系统，或 `EORZEA_FONT` 指定文件；
不打包大字体，不依赖开发者机器的绝对路径。

```bash
cargo check --locked --workspace --all-targets -j2
cargo check --locked -p eorzea-auth --features se -j2
cargo test --locked -p eorzea-app -j2 -- --test-threads=2
cargo build --locked --release -p eorzea-app -j2
```

release 启用 thin LTO、单 codegen unit 和符号裁剪，保留默认运行优化及 unwind 语义。
`target/debug` 含调试信息；文件大小和 RSS/PSS 不是同一指标。无窗口测试不提供正式程序的空闲 RAM/CPU 数据。

## 结构

- `main.rs`：Iced 入口、窗口配置、系统字体；Windows release 无额外控制台窗口。
- `ui/mod.rs`：根状态、消息处理、导航/状态栏/模态框、关闭保护、Iced executor 与通知订阅。
- `ui/backend.rs`：认证、账号/配置持久化、大区请求、GameFileManager 更新、Launcher 启动。
- `ui/state.rs`：草稿校验、登录任务 generation 和根操作状态。
- `ui/home.rs`：游戏位置/版本、Dalamud/Wine 摘要、检查/恢复更新、安装/下载双进度、启动。
- `ui/footer.rs`：全局版本/最新日志底栏、可滚动日志抽屉和独立操作提示。
- `ui/log_view.rs`：只读原生日志文本区、跨行选择/复制、暂停跟随与日志级别高亮。
- `ui/motion.rs`：基于 Iced `Animation` 的控件内位移/按下反馈，以及只读文本区的 IME/光标刷新适配。
- `ui/login.rs`：账号默认/删除确认，真实扫码/推送/密码、取消与保存失败重试。
- `ui/settings.rs`：跨页草稿、下载/Wine/Dalamud 字段、目录选择、固定保存栏。
- `ui/theme.rs`：白色/zinc 风格、72px 图标侧栏对应控件、黑白主操作和系统字体。
- `logging.rs` / `clipboard.rs`：事件唤醒的 tracing 缓冲、最新在上显示、原始时间顺序复制；
  剪贴板所有者保持在 UI 线程。

Iced executor 和业务任务共用两个 Tokio worker。配置/设备初始化和写盘放 `spawn_blocking`；
网络流程正常 await。低频结果使用 channel，更新及启动进度各只保留最新快照；`Notify` 的许可
在 UI 尚未订阅时也会保留。日志与任务事件唤醒订阅，不使用定时空闲刷新或系统硬件清查。
切页不丢设置草稿、不取消启动或真实补丁事务。焦点输入框的光标闪烁等控件行为由 Iced 管理。

## 底栏与动效

36px 底栏横跨窗口：左侧为 `Eorzea <CARGO_PKG_VERSION>`（启动器版本，不是游戏版本），右侧为
最新一条真实日志、全文提示和展开按钮。没有日志时回显当前状态。展开 240px 的全局日志抽屉，
覆盖内容底部、不重排主页面；可滚动/复制，最新在上，复制仍按原始时间顺序。切页保留抽屉状态，
Escape 优先关闭模态框或下拉，再收起日志。登录等模态框在抽屉上层，不允许穿透操作。

日志文本区支持跨行拖选、Ctrl/Cmd+A 和 Ctrl/Cmd+C。复制所选按可见选区顺序，工具栏「复制全部」
仍按原始时间从旧到新复制当前保留日志。文本区只接受选择、移动和滚动，不能编辑、粘贴或剪切；
阅读时暂停文本区跟随，避免新日志覆盖选区/滚动位置，底栏仍继续更新。点击「跟随最新」清除选区并恢复最新列表。
日志的 INFO/WARN/ERROR 颜色由轻量高亮器保留，多行详情继承所属记录的级别，不引入语法高亮库。
只读适配不请求 IME，也不为光标闪烁安排持续重绘；无窗口剪贴板测试使用内存替身，不访问桌面剪贴板。

操作结果显示在独立的 6 秒提示中，可手动关闭，通知带 generation 防止旧定时结果清掉新提示；
它不是日志缓冲的一部分，因此不会被新日志覆盖。更新/设置/登录的原位错误仍保留。

抽屉为 200ms ease-out 位移，按钮为 100ms 的轻微按下反馈。动画状态保存在 widget tree，
通过 `Shell::request_redraw_at` 临时请求绘制，不产生逐帧应用消息，也不触发布局失效；结束后停止请求。
只对绘制做位移/缩放与裁剪，不动画化页面高度，不做模糊、循环或持续呼吸效果。
固定裁剪边界通过逆变换传入子控件视口，确保嵌套文字、选区和按钮图层在抽屉位移时仍停在底栏上方；
不能只依赖外层 renderer layer，因为 Iced 的新裁剪图层不会自动与父层裁剪相交。
设置环境变量 `EORZEA_REDUCED_MOTION=1` 可关闭动效。能力、限制与来源见 [Iced 动效调查](gui-motion.md)。
按钮的普通/hover/pressed/disabled 样式分别定义，悬停反馈不依赖动画；禁用按钮不出现 hover 高亮。
56px 启动按钮的内容容器在两个方向填满可用空间后居中，避免固定按钮高度与收缩内容导致文字上偏。

## 业务与安全

启动复用 `Launcher::launch_account`：预检 → Dalamud 准备 → 最新磁盘凭据 → 自动登录 →
保存轮换 key → 创建游戏进程。不重写认证协议，不跨启动尝试缓存一次性游戏 ticket。
更新复用 `pending_update` / `check_update` / `update_with_min_concurrency`，保留核心的恢复队列、锁和版本提交。

检查/更新/启动互斥，账号和配置保存也禁止与关键操作并发。普通关闭请求不自动退出：关键工作进行中
可选择完成后退出；若仍有草稿则再确认。补丁提交没有任意 abort 按钮，强杀/断电依赖核心恢复机制。
与外部 CLI 的原子账号写入互斥等已知缺口仍见 `TODO.md`。

登录关闭/切换方式会清除密码、abort 本地任务并递增 generation，旧 QR/推送/登录结果不能污染新会话。
密码框使用 Iced secure input，提交后从根状态移走，用 `Zeroizing` 在任务结束/取消时清除；
不写入配置或日志，消息 Debug 不输出字段值。保存失败不关闭弹窗、不更新账号镜像，保留认证结果供原位重试。

设置保存基于最新磁盘配置，仅覆盖表单拥有的字段，保留未展示的 Wine 环境变量、DXVK HUD/帧率及旧 area。
Windows 不展示 Wine，Linux/macOS 分别展示现有平台的同步能力。并发数和毫秒延迟非法时禁止保存且保留 dirty 状态。
跨平台未知配置字段保真与 Wine 精确无副作用摘要等缺口没有因换前端而宣称解决。

## 无窗口验证边界

测试只创建隔离根状态、内存 Iced widget tree 与 CPU renderer，不调用生产初始化、窗口 event loop、
compositor、portal 或原生输入注入。覆盖三个页面及模态框的最小/默认尺寸布局与绘制命令、真实 secure input
合成键盘事件、草稿/字段校验、隐藏配置保留、操作互斥、取消登录、保存失败恢复、最新进度/终态顺序、
通知许可、退出保护和日志复制格式；并覆盖底栏预览、切页保留抽屉、独立提示过期隔离、动效中途反转、
减少动效、过渡不使布局失效及结束后停止请求重绘、原生日志拖选/复制与拒绝编辑、日志 glyph 实际像素可见性、
启动按钮双轴居中与 hover 实际像素变化。抽屉动画像素回归复用同一控件树及每段过渡的布局，检查普通/150% 缩放下
收起、完全关闭、重开和中途反转时底栏不受文字/选区/控件影响，结束帧与静态参考一致。账号保存测试只写测试临时目录。

这些测试不等于真实窗口/中文 IME/剪贴板/portal/无障碍/Windows/macOS 验收，也不代替真实认证、
发行补丁和游戏启动端到端验证。不得擅自在用户工作桌面开 GUI、截图、切工作区或抢焦点；
原生验证需要用户明确许可或真正隔离的 compositor/headless 环境。

无窗口像素图只用于检查布局与配色，不是桌面截图，也不证明原生动效帧率或运行内存。
