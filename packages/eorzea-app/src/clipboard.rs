//! 显式复制日志，不依赖原生渲染器的文本选择支持。

pub fn copy_text(text: String) -> Result<(), String> {
    use std::cell::RefCell;

    // Linux 上剪贴板所有者需要保持存活，不能写入后立即释放。
    thread_local! {
        static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
    }
    CLIPBOARD.with(|clipboard| {
        let mut clipboard = clipboard.borrow_mut();
        if clipboard.is_none() {
            *clipboard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
        }
        clipboard
            .as_mut()
            .unwrap()
            .set_text(text)
            .map_err(|e| e.to_string())
    })
}
