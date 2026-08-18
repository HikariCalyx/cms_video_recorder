# Toolbar
btn-record = 錄製
btn-stop = 停止 { $clock }
btn-preparing = 準備中
btn-saving = 儲存中

# Window picker
picker-choose-window = 選擇視窗
picker-window-count = { $count } 個視窗
picker-show-others = 顯示其他視窗
picker-no-windows = 未找到可擷取的視窗
picker-no-maplestory = 未找到置於前景的新楓之谷視窗
picker-minimized = 已最小化
picker-clear-selection = 清除選擇
btn-refresh = 重新整理

# Settings panel
panel-settings = 設定
btn-done = 完成
field-output-dir = 儲存位置
field-max-duration = 最長錄製時間
field-record-hotkey = 錄製 / 停止快捷鍵
field-language = 語言
btn-browse = 瀏覽…
hint-duration-capped = 秒後自動停止
hint-duration-unlimited = 秒，0 表示不限時長
hint-hotkey-capturing = 按下新的組合鍵；Esc 取消，Backspace 清除
hint-hotkey-global = 全域有效，遊戲視窗在前景也能觸發
btn-change = 修改
btn-clear = 清除
btn-cancel = 取消
hotkey-waiting = 等待按鍵…
hotkey-unset = 未設定
label-config-file = 設定檔: { $path }

# Recordings manager
videos-count = { $count } 個影片
videos-empty = 暫無錄製影片
btn-play = 播放
btn-compress = 壓縮
btn-show-folder = 瀏覽
btn-delete = 刪除
confirm-delete = 確認刪除?
status-compressing = 壓縮中…
status-saved = 已儲存 { $name }
status-deleted = 已刪除 { $name }

# Status messages
status-pick-window = 請先選擇視窗
status-stop-before-compress = 請先停止錄製再壓縮
status-already-compressing = 已有影片正在壓縮
status-compress-cancelled = 已取消壓縮
status-compressed = 已壓縮至 { $size }
status-compressed-partial = 已壓縮至 { $size }（未達 5000KB）
word-hotkey = 快捷鍵
word-unknown-error = 未知錯誤

# Native dialogs
dialog-pick-folder = 選擇儲存位置
dialog-save-compressed = 儲存壓縮影片

# MapleStory server aliases
alias-live = 新楓之谷
alias-test = 新楓之谷測試服
alias-m = 楓之谷M
alias-n = 楓之谷N
alias-classic = 新楓之谷：經典版
alias-worlds = 楓之谷世界

# Errors
error-no-appdata = 無法定位 APPDATA
error-io = 檔案寫入失敗: { $msg }
error-encoder = 編碼失敗: { $msg }
error-capture = 錄製失敗: { $msg }
error-audio = 音訊錄製失敗: { $msg }
error-unsupported-platform = 目前平台不支援
error-unsupported-recording = 目前平台不支援錄製
error-record-thread = 錄製執行緒意外結束
error-settings-save = 設定未儲存: { $error }
error-play = 播放失敗: { $error }
error-open = 開啟失敗: { $error }
error-delete = 刪除失敗: { $error }
error-compress = 壓縮失敗: { $error }
error-compress-thread = 壓縮執行緒意外結束
error-ffmpeg-missing = 未找到 ffmpeg.exe，請確認它與程式放在一起
error-hotkey-taken = { $combination } 已被使用
error-copy-file = 複製檔案失敗: { $error }
error-read-result = 讀取壓縮結果失敗: { $error }
error-replace-file = 取代原檔案失敗: { $error }
error-ffmpeg-start = 無法啟動 ffmpeg: { $error }
error-ffmpeg-wait = 等待 ffmpeg 結束失敗: { $error }
error-ffmpeg-encode = ffmpeg 編碼失敗: { $error }
error-compress-state = 壓縮狀態損壞
error-shellexecute = ShellExecute 傳回 { $code }
error-target-closed = 目標視窗已關閉
error-target-size = 目標視窗尺寸無效
error-audio-thread = 音訊執行緒意外結束
error-com-init = COM 初始化失敗
error-enum-devices = 列舉音訊裝置失敗
error-default-device = 未找到預設播放裝置
error-activate-client = 啟用音訊用戶端失敗
error-mix-format = 讀取音訊格式失敗
error-init-capture = 初始化音訊擷取失敗
error-capture-interface = 取得音訊擷取介面失敗
error-start-capture = 啟動音訊擷取失敗
error-read-packet = 讀取音訊封包失敗: { $error }
error-get-buffer = 取得音訊緩衝失敗: { $error }
error-release-buffer = 釋放音訊緩衝失敗: { $error }
error-audio-format = 音訊格式無效
error-float-bit-depth = 不支援的浮點位元深度: { $bytes }
