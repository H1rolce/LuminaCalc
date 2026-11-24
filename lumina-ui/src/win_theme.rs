use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemTheme {
    pub dark: bool,
    pub accent: Color32,
}

impl Default for SystemTheme {
    fn default() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(0x60, 0xCD, 0xFF),
        }
    }
}

pub fn read_system_theme() -> SystemTheme {
    #[cfg(target_os = "windows")]
    {
        let dark = read_hkcu_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "AppsUseLightTheme",
        )
        .is_none_or(|v| v == 0);

        let accent = read_hkcu_dword(r"Software\Microsoft\Windows\DWM", "AccentColor")
            .map(|abgr| {
                let r = (abgr & 0xFF) as u8;
                let g = ((abgr >> 8) & 0xFF) as u8;
                let b = ((abgr >> 16) & 0xFF) as u8;
                Color32::from_rgb(r, g, b)
            })
            .or_else(|| {
                read_hkcu_dword(r"Software\Microsoft\Windows\DWM", "ColorizationColor").map(|argb| {
                    let r = ((argb >> 16) & 0xFF) as u8;
                    let g = ((argb >> 8) & 0xFF) as u8;
                    let b = (argb & 0xFF) as u8;
                    Color32::from_rgb(r, g, b)
                })
            })
            .unwrap_or(SystemTheme::default().accent);

        SystemTheme { dark, accent }
    }

    #[cfg(not(target_os = "windows"))]
    {
        SystemTheme::default()
    }
}

#[inline]
pub fn luminance(c: Color32) -> f32 {
    (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / 255.0
}

pub fn tune_accent_for_mode(accent: Color32, dark: bool) -> Color32 {
    let lum = luminance(accent);
    if dark && lum < 0.34 {
        let t = ((0.38 - lum) * 1.4).clamp(0.15, 0.52);
        crate::style::Palette::lerp(accent, Color32::WHITE, t)
    } else if !dark && lum > 0.66 {
        let t = ((lum - 0.58) * 1.3).clamp(0.15, 0.48);
        crate::style::Palette::lerp(accent, Color32::BLACK, t)
    } else {
        accent
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn read_hkcu_dword(subkey: &str, value: &str) -> Option<u32> {
    const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as isize;
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            hkey: isize,
            lp_sub_key: *const u16,
            lp_value: *const u16,
            dw_flags: u32,
            pdw_type: *mut u32,
            pv_data: *mut u8,
            pcb_data: *mut u32,
        ) -> i32;
    }

    let subkey_w = to_wide(subkey);
    let value_w = to_wide(value);
    let mut data: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;

    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            value_w.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast::<u8>(),
            &mut size,
        )
    };
    if status == 0 && size == 4 {
        Some(data)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_system_theme_safely() {
        let sys = read_system_theme();
        let tuned = tune_accent_for_mode(sys.accent, sys.dark);
        let lum = luminance(tuned);
        if sys.dark {
            assert!(lum >= 0.25, "dark mode accent too dark: {lum}");
        } else {
            assert!(lum <= 0.75, "light mode accent too bright: {lum}");
        }
    }
}
