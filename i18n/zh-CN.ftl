# Toolbar
btn-record = 录制
btn-stop = 停止 { $clock }
btn-preparing = 准备中
btn-saving = 保存中

# Window picker
picker-choose-window = 选择窗口
picker-window-count = { $count } 个窗口
picker-show-others = 显示其他窗口
picker-no-windows = 未找到可捕获窗口
picker-no-maplestory = 未找到置于前台的冒险岛窗口
picker-minimized = 已最小化
picker-clear-selection = 清除选择
btn-refresh = 刷新

# Settings panel
panel-settings = 设置
btn-done = 完成
field-output-dir = 保存位置
field-max-duration = 最长录制时间
field-record-hotkey = 录制 / 停止快捷键
field-language = 语言
btn-browse = 浏览…
hint-duration-capped = 秒后自动停止
hint-duration-unlimited = 秒，0 表示不限时长
hint-hotkey-capturing = 按下新的组合键；Esc 取消，Backspace 清除
hint-hotkey-global = 全局有效，游戏窗口在前台也能触发
btn-change = 修改
btn-clear = 清除
btn-cancel = 取消
hotkey-waiting = 等待按键…
hotkey-unset = 未设置
label-config-file = 配置文件: { $path }

# Recordings manager
videos-count = { $count } 个视频
videos-empty = 暂无录制视频
btn-play = 播放
btn-compress = 压缩
btn-show-folder = 浏览
btn-delete = 删除
confirm-delete = 确认删除?
status-compressing = 压缩中…
status-saved = 已保存 { $name }
status-deleted = 已删除 { $name }

# Status messages
status-pick-window = 请先选择窗口
status-stop-before-compress = 请先停止录制再压缩
status-already-compressing = 已有视频正在压缩
status-compress-cancelled = 已取消压缩
status-compressed = 已压缩到 { $size }
status-compressed-partial = 已压缩到 { $size }（未达 5000KB）
word-hotkey = 快捷键
word-unknown-error = 未知错误

# Native dialogs
dialog-pick-folder = 选择保存位置
dialog-save-compressed = 保存压缩视频

# MapleStory server aliases
alias-live = 冒险岛正式服
alias-test = 冒险岛测试服
alias-m = 冒险岛M
alias-n = 冒险岛N
alias-classic = 冒险岛怀旧服
alias-worlds = 冒险岛世界

# Errors
error-no-appdata = 无法定位 APPDATA
error-io = 文件写入失败: { $msg }
error-encoder = 编码失败: { $msg }
error-capture = 录制失败: { $msg }
error-audio = 音频录制失败: { $msg }
error-unsupported-platform = 当前平台不支持
error-unsupported-recording = 当前平台不支持录制
error-record-thread = 录制线程意外结束
error-settings-save = 设置未保存: { $error }
error-play = 播放失败: { $error }
error-open = 打开失败: { $error }
error-delete = 删除失败: { $error }
error-compress = 压缩失败: { $error }
error-compress-thread = 压缩线程意外结束
error-ffmpeg-missing = 未找到 ffmpeg.exe，请确认它与程序放在一起
error-hotkey-taken = { $combination } 已被占用
error-copy-file = 复制文件失败: { $error }
error-read-result = 读取压缩结果失败: { $error }
error-replace-file = 替换原文件失败: { $error }
error-ffmpeg-start = 无法启动 ffmpeg: { $error }
error-ffmpeg-wait = 等待 ffmpeg 结束失败: { $error }
error-ffmpeg-encode = ffmpeg 编码失败: { $error }
error-compress-state = 压缩状态损坏
error-shellexecute = ShellExecute 返回 { $code }
error-target-closed = 目标窗口已关闭
error-target-size = 目标窗口尺寸无效
error-audio-thread = 音频线程意外结束
error-com-init = COM 初始化失败
error-enum-devices = 枚举音频设备失败
error-default-device = 未找到默认播放设备
error-activate-client = 激活音频客户端失败
error-mix-format = 读取音频格式失败
error-init-capture = 初始化音频采集失败
error-capture-interface = 获取音频采集接口失败
error-start-capture = 启动音频采集失败
error-read-packet = 读取音频包失败: { $error }
error-get-buffer = 获取音频缓冲失败: { $error }
error-release-buffer = 释放音频缓冲失败: { $error }
error-audio-format = 音频格式无效
error-float-bit-depth = 不支持的浮点位深: { $bytes }
