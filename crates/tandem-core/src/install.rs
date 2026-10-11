//! How this copy of the app was installed, where it decides behaviour: a
//! Microsoft Store (MSIX) install is updated by the Store, never by the app's
//! own updater (D-1713).

/// The full name of the MSIX package this process runs from, or `None` for
/// an unpackaged install (NSIS, MSI, a dev build) and on other platforms.
pub fn windows_package_full_name() -> Option<String> {
    #[cfg(windows)]
    {
        windows_impl::current_package_full_name()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// True when the Microsoft Store owns this install's updates.
pub fn managed_by_store(package_full_name: Option<&str>) -> bool {
    package_full_name.is_some_and(|name| !name.trim().is_empty())
}

#[cfg(windows)]
mod windows_impl {
    use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    pub fn current_package_full_name() -> Option<String> {
        let mut len = 0u32;
        // SAFETY: the first call only reports the length (null buffer); the
        // second gets a buffer of exactly that many UTF-16 units.
        unsafe {
            // APPMODEL_ERROR_NO_PACKAGE when the process isn't packaged.
            if GetCurrentPackageFullName(&mut len, std::ptr::null_mut())
                != ERROR_INSUFFICIENT_BUFFER
            {
                return None;
            }
            let mut buf = vec![0u16; len as usize];
            if GetCurrentPackageFullName(&mut len, buf.as_mut_ptr()) != ERROR_SUCCESS {
                return None;
            }
            let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_sh_06_a_store_package_owns_its_updates() {
        assert!(managed_by_store(Some(
            "Vibezeo.Vibezeo_0.5.0.0_x64__8wekyb3d8bbwe"
        )));
        assert!(!managed_by_store(Some("")));
        assert!(!managed_by_store(None));
    }

    /// Tests run unpackaged: on Windows (and under Wine) the real call says
    /// so, elsewhere there's no package at all.
    #[test]
    fn f_sh_06_an_unpackaged_process_has_no_package_name() {
        assert_eq!(windows_package_full_name(), None);
    }
}
