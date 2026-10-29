use crate::config::Provider;

fn target(provider: Provider) -> &'static str {
    match provider {
        Provider::Gemini => "Speeching/Gemini",
        Provider::OpenAi => "Speeching/OpenAI",
    }
}

pub fn load(provider: Provider) -> Result<Option<String>, String> {
    platform::read(target(provider))
}
pub fn save(provider: Provider, key: &str) -> Result<(), String> {
    platform::write(target(provider), key)
}
pub fn forget(provider: Provider) -> Result<(), String> {
    platform::delete(target(provider))
}

#[cfg(windows)]
mod platform {
    use std::{ffi::c_void, io, ptr};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    fn failed(action: &str) -> String {
        format!("{action}: {}", io::Error::last_os_error())
    }
    fn not_found() -> bool {
        io::Error::last_os_error().raw_os_error() == Some(1168)
    }

    pub fn read(name: &str) -> Result<Option<String>, String> {
        let name = wide(name);
        let mut entry: *mut CREDENTIALW = ptr::null_mut();
        // Windows allocates the returned credential; copy its blob before CredFree.
        unsafe {
            if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut entry) == 0 {
                return if not_found() {
                    Ok(None)
                } else {
                    Err(failed("Could not read saved API key"))
                };
            }
            let value = if (*entry).CredentialBlobSize == 0 {
                Ok(String::new())
            } else if (*entry).CredentialBlob.is_null() {
                Err("Saved API key is invalid.".into())
            } else {
                let bytes = std::slice::from_raw_parts(
                    (*entry).CredentialBlob,
                    (*entry).CredentialBlobSize as usize,
                );
                String::from_utf8(bytes.to_vec())
                    .map_err(|_| "Saved API key is invalid.".to_string())
            };
            CredFree(entry.cast::<c_void>());
            value.map(Some)
        }
    }
    pub fn write(name: &str, value: &str) -> Result<(), String> {
        if value.trim().is_empty() {
            return Err("Enter an API key.".into());
        }
        let bytes = value.trim().as_bytes();
        if bytes.len() > 2560 {
            return Err("API key is too long to save.".into());
        }
        let mut name = wide(name);
        let entry = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        // CredWriteW copies the key into Windows Credential Manager.
        if unsafe { CredWriteW(&entry, 0) } == 0 {
            Err(failed("Could not save API key"))
        } else {
            Ok(())
        }
    }
    pub fn delete(name: &str) -> Result<(), String> {
        let name = wide(name);
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 || not_found() {
            Ok(())
        } else {
            Err(failed("Could not remove saved API key"))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn credential_round_trip_uses_an_isolated_test_entry() {
            let name = format!(
                "Speeching/test/{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            assert_eq!(read(&name).unwrap(), None);
            write(&name, "example-key-for-testing").unwrap();
            assert_eq!(
                read(&name).unwrap().as_deref(),
                Some("example-key-for-testing")
            );
            delete(&name).unwrap();
            assert_eq!(read(&name).unwrap(), None);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn read(_: &str) -> Result<Option<String>, String> {
        Err("Secure API key storage requires Windows Credential Manager.".into())
    }
    pub fn write(_: &str, _: &str) -> Result<(), String> {
        Err("Secure API key storage requires Windows Credential Manager.".into())
    }
    pub fn delete(_: &str) -> Result<(), String> {
        Err("Secure API key storage requires Windows Credential Manager.".into())
    }
}
