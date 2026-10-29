use crate::config::Provider;
use std::time::Duration;

const RETRY_DELAYS_MS: [u64; 5] = [2, 5, 10, 20, 40];

fn retry_read(
    mut read: impl FnMut() -> Result<Option<String>, String>,
    accept: impl Fn(&str) -> bool,
    mut wait: impl FnMut(Duration),
) -> Result<Option<String>, String> {
    for attempt in 0..=RETRY_DELAYS_MS.len() {
        if attempt > 0 {
            wait(Duration::from_millis(RETRY_DELAYS_MS[attempt - 1]));
        }
        if let Some(value) = read()? {
            if accept(&value) {
                return Ok(Some(value));
            }
        }
    }
    Ok(None)
}

fn retry_delete(
    mut delete: impl FnMut() -> Result<bool, String>,
    mut wait: impl FnMut(Duration),
) -> Result<(), String> {
    for attempt in 0..=RETRY_DELAYS_MS.len() {
        if attempt > 0 {
            wait(Duration::from_millis(RETRY_DELAYS_MS[attempt - 1]));
        }
        if delete()? {
            return Ok(());
        }
    }
    Ok(())
}

#[derive(Clone)]
pub struct Store {
    gemini: String,
    openai: String,
    #[cfg(test)]
    empty_test_store: bool,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            gemini: "Speeching/Gemini".into(),
            openai: "Speeching/OpenAI".into(),
            #[cfg(test)]
            empty_test_store: false,
        }
    }
}

impl Store {
    #[cfg(test)]
    pub fn isolated(namespace: &str) -> Self {
        Self {
            gemini: format!("{namespace}/Gemini"),
            openai: format!("{namespace}/OpenAI"),
            empty_test_store: false,
        }
    }

    #[cfg(test)]
    pub fn empty_for_test() -> Self {
        Self {
            gemini: String::new(),
            openai: String::new(),
            empty_test_store: true,
        }
    }

    fn target(&self, provider: Provider) -> &str {
        match provider {
            Provider::Gemini => &self.gemini,
            Provider::OpenAi => &self.openai,
        }
    }

    pub fn load(&self, provider: Provider) -> Result<Option<String>, String> {
        #[cfg(test)]
        if self.empty_test_store {
            return Ok(None);
        }
        retry_read(
            || platform::read(self.target(provider)),
            |_| true,
            std::thread::sleep,
        )
    }
    pub fn save(&self, provider: Provider, key: &str) -> Result<(), String> {
        #[cfg(test)]
        if self.empty_test_store {
            return Err("The empty test credential store cannot save keys.".into());
        }
        platform::write(self.target(provider), key)?;
        match retry_read(
            || platform::read(self.target(provider)),
            |saved| saved == key.trim(),
            std::thread::sleep,
        )? {
            Some(_) => Ok(()),
            None => Err("Could not verify saved API key.".into()),
        }
    }
    pub fn forget(&self, provider: Provider) -> Result<(), String> {
        #[cfg(test)]
        if self.empty_test_store {
            return Err("The empty test credential store cannot remove keys.".into());
        }
        retry_delete(
            || platform::delete(self.target(provider)),
            std::thread::sleep,
        )
    }
}

#[cfg(windows)]
mod platform {
    use std::{ffi::c_void, io, ptr, sync::Mutex};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    // Concurrent Windows credential operations produced missing readbacks under load.
    // Serialize individual calls within this process; retries still happen outside the lock.
    static CREDENTIAL_ACCESS: Mutex<()> = Mutex::new(());

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
        let _guard = CREDENTIAL_ACCESS
            .lock()
            .unwrap_or_else(|error| error.into_inner());
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
        let _guard = CREDENTIAL_ACCESS
            .lock()
            .unwrap_or_else(|error| error.into_inner());
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
    pub fn delete(name: &str) -> Result<bool, String> {
        let _guard = CREDENTIAL_ACCESS
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let name = wide(name);
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
            Ok(true)
        } else if not_found() {
            Ok(false)
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
            let mut entry: *mut CREDENTIALW = ptr::null_mut();
            let wide_name = wide(&name);
            assert_ne!(
                unsafe { CredReadW(wide_name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut entry) },
                0
            );
            let persistence = unsafe { (*entry).Persist };
            unsafe { CredFree(entry.cast::<c_void>()) };
            assert_eq!(
                read(&name).unwrap().as_deref(),
                Some("example-key-for-testing")
            );
            delete(&name).unwrap();
            assert_eq!(persistence, CRED_PERSIST_LOCAL_MACHINE);
            assert_eq!(read(&name).unwrap(), None);
        }

        #[test]
        fn credential_survives_separate_processes() {
            const PHASE: &str = "SPEECHING_CREDENTIAL_TEST_PHASE";
            const NAMESPACE: &str = "SPEECHING_CREDENTIAL_TEST_NAMESPACE";
            const KEY: &str = "synthetic-process-restart-key";
            if let Ok(phase) = std::env::var(PHASE) {
                let store = crate::credentials::Store::isolated(&std::env::var(NAMESPACE).unwrap());
                match phase.as_str() {
                    "write" => store.save(crate::config::Provider::Gemini, KEY).unwrap(),
                    "read" => assert_eq!(
                        store
                            .load(crate::config::Provider::Gemini)
                            .unwrap()
                            .as_deref(),
                        Some(KEY)
                    ),
                    _ => panic!("Unknown test phase"),
                }
                return;
            }
            let namespace = format!(
                "Speeching/test/process-restart/{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let store = crate::credentials::Store::isolated(&namespace);
            let run = |phase| {
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "credentials::platform::tests::credential_survives_separate_processes",
                    ])
                    .env(PHASE, phase)
                    .env(NAMESPACE, &namespace)
                    .status()
                    .unwrap()
            };
            let write = run("write");
            let read = run("read");
            let cleanup = store.forget(crate::config::Provider::Gemini);
            assert!(write.success());
            assert!(read.success());
            cleanup.unwrap();
        }

        #[test]
        fn parallel_synthetic_credentials_are_immediately_retrievable() {
            let namespace = format!(
                "Speeching/test/concurrent/{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
            let threads: Vec<_> = (0..8)
                .map(|thread| {
                    let namespace = namespace.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        let mut errors = Vec::new();
                        for index in 0..20 {
                            let store = crate::credentials::Store::isolated(&format!(
                                "{namespace}/{thread}/{index}"
                            ));
                            let result = store
                                .save(crate::config::Provider::Gemini, "synthetic-concurrent-key");
                            let later = store.load(crate::config::Provider::Gemini);
                            let cleanup = store.forget(crate::config::Provider::Gemini);
                            if result.is_err() || !matches!(later, Ok(Some(_))) || cleanup.is_err()
                            {
                                errors.push(format!(
                                    "save={:?}, later={:?}, cleanup={:?}",
                                    result.err(),
                                    later.map(|value| value.is_some()),
                                    cleanup.err()
                                ));
                            }
                        }
                        errors
                    })
                })
                .collect();
            let errors: Vec<_> = threads
                .into_iter()
                .flat_map(|thread| thread.join().unwrap())
                .collect();
            assert!(errors.is_empty(), "{errors:?}");
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn retries_missing_and_stale_readback_before_accepting_a_value() {
        let mut values = [None, Some("old".to_owned()), Some("new".to_owned())].into_iter();
        let mut waits = Vec::new();
        let found = retry_read(
            || Ok(values.next().unwrap()),
            |value| value == "new",
            |delay| waits.push(delay),
        )
        .unwrap();
        assert_eq!(found.as_deref(), Some("new"));
        assert_eq!(waits, [Duration::from_millis(2), Duration::from_millis(5)]);
    }

    #[test]
    fn retries_a_missing_deletion_and_propagates_read_errors() {
        let mut deletes = [false, false, true].into_iter();
        let mut waits = Vec::new();
        retry_delete(|| Ok(deletes.next().unwrap()), |delay| waits.push(delay)).unwrap();
        assert_eq!(waits.len(), 2);
        let error = retry_read(
            || Err("blocked".into()),
            |_| true,
            |_| panic!("must not retry errors"),
        );
        assert_eq!(error.unwrap_err(), "blocked");
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
    pub fn delete(_: &str) -> Result<bool, String> {
        Err("Secure API key storage requires Windows Credential Manager.".into())
    }
}
