//! What the app calls itself.
//!
//! The project is MapleView, but the UI is Chinese, so on a Chinese system the
//! name shown to the user is 枫阅. Everything user-facing goes through
//! [`display_name`] so the two can never drift apart.

use std::sync::OnceLock;

/// The name shown in the window title, the welcome page and the About dialog.
pub fn display_name() -> &'static str {
    static NAME: OnceLock<&'static str> = OnceLock::new();
    NAME.get_or_init(|| {
        if chinese_locale() {
            "枫阅"
        } else {
            "MapleView"
        }
    })
}

fn chinese_locale() -> bool {
    locale().is_some_and(|name| is_chinese(&name))
}

/// Chinese is written `zh` in every locale name we can be handed: `zh-CN`,
/// `zh_TW`, `zh-Hans-CN`, and so on.
fn is_chinese(locale: &str) -> bool {
    locale.trim_start().to_ascii_lowercase().starts_with("zh")
}

/// The OS's default locale name.
///
/// `LANG`/`LC_ALL` are not usable as the primary signal on Windows: a shell can
/// run with `LANG=C.UTF-8` while the system itself is Chinese, which is exactly
/// the case this was written for.
#[cfg(target_os = "windows")]
fn locale() -> Option<String> {
    /// `LOCALE_NAME_MAX_LENGTH` from the Win32 documentation.
    const MAX: usize = 85;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
    }

    let mut buffer = [0u16; MAX];
    // SAFETY: the buffer holds MAX UTF-16 units, which is the documented limit
    // for this call, and the length we pass matches the buffer exactly.
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), MAX as i32) };
    if written <= 0 {
        return None;
    }
    // The return value counts the terminating NUL, which is not part of the name.
    Some(String::from_utf16_lossy(&buffer[..written as usize - 1]))
}

#[cfg(not(target_os = "windows"))]
fn locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::is_chinese;

    #[test]
    fn recognises_chinese_locales() {
        for name in ["zh", "zh-CN", "zh_CN.UTF-8", "ZH-Hans-CN", " zh-TW"] {
            assert!(is_chinese(name), "{name} should read as Chinese");
        }
    }

    #[test]
    fn leaves_other_locales_alone() {
        for name in ["", "C", "C.UTF-8", "en-US", "ja_JP.UTF-8", "ko-KR"] {
            assert!(!is_chinese(name), "{name} should not read as Chinese");
        }
    }
}
