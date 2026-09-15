//! BYOK provider keys, stored in the OS keychain and nowhere else.
//!
//! Spec the data policy: keys live in the OS keychain, never in a config file, never
//! logged, never sent anywhere but the provider's own API. That rules out the
//! obvious shortcut of a `settings.json` next to the Parquet cache, so this
//! module is the only place in the codebase that holds a secret.
//!
//! Two rules follow from "never logged", and both are enforced here rather
//! than trusted to callers:
//!
//! * no function in this module returns the key inside an error. The keyring
//!   crate's own [`Display`](std::fmt::Display) was checked for the same
//!   (`BadEncoding` prints "Data is not UTF-8 encoded" and withholds the
//!   bytes), so passing its message through is safe.
//! * nothing here implements `Debug`/`Display` over a key, so a stray
//!   `{:?}` on a struct cannot leak one.

use replay_core::{Error, Result};

/// The keychain "service" every entry is filed under. Users see this string in
/// Credential Manager / Keychain Access, so it is the product name, and the
/// per-provider adapter id is the account.
const SERVICE: &str = "bar-replay";

fn entry(provider: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, provider)
        .map_err(|e| Error::Provider(format!("keychain unavailable for {provider}: {e}")))
}

/// The stored key for an adapter, or `None` if there isn't a usable one.
///
/// Every failure collapses to `None` on purpose. A locked keychain, a headless
/// Linux box with no Secret Service, or a user who simply never added a key are
/// all the same situation for the caller: this provider cannot be used right
/// now, and the free providers still can (the data policy — BYOK is an upgrade path,
/// never a requirement). Reporting them separately would only give callers a
/// way to turn a missing key into a crash.
pub fn get(provider: &str) -> Option<String> {
    let key = keyring::Entry::new(SERVICE, provider)
        .ok()?
        .get_password()
        .ok()?;
    // A blank entry is a half-finished paste in the settings dialog, not a
    // key; treating it as one would send an empty credential to the provider
    // and earn a confusing 401.
    if key.trim().is_empty() {
        return None;
    }
    Some(key)
}

/// Store (or replace) the key for an adapter.
pub fn set(provider: &str, key: &str) -> Result<()> {
    if key.trim().is_empty() {
        return Err(Error::Provider(format!(
            "the {provider} API key is empty; paste the key from your {provider} account, or remove it to stop using {provider}"
        )));
    }
    entry(provider)?
        .set_password(key)
        .map_err(|e| Error::Provider(format!("could not save the {provider} API key: {e}")))
}

/// Forget the key for an adapter.
///
/// Deleting a key that was never there is success, not an error: the user asked
/// for "no key stored for this provider" and that is the state they end up in.
pub fn delete(provider: &str) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::Provider(format!(
            "could not remove the {provider} API key: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI is a headless Linux runner with no Secret Service, so this is also
    /// the check that a missing *store* behaves like a missing *key*.
    #[test]
    fn an_unknown_provider_reads_back_as_none_and_never_panics() {
        assert!(get("provider-that-does-not-exist-6f1a2b").is_none());
    }

    #[test]
    fn an_empty_key_is_refused_before_it_reaches_the_keychain() {
        let e = set("databento", "   ").unwrap_err();
        assert!(matches!(e, Error::Provider(_)));
    }

    /// The guarantee that no message carries the secret is structural rather
    /// than testable: `key` is never an argument to any `format!` in this
    /// module. A test cannot prove that, but a `grep` for `{key}` can, and
    /// deliberately writing a real secret into the developer's own keychain to
    /// inspect a failure message would be a worse trade than the grep.
    #[test]
    fn the_blank_key_message_tells_the_user_what_to_do() {
        let msg = set("databento", "").unwrap_err().to_string();
        assert!(msg.contains("databento"), "{msg}");
        assert!(msg.contains("paste the key"), "{msg}");
    }
}
