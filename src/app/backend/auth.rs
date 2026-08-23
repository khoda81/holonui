use base64::{Engine as _, engine::general_purpose::URL_SAFE};
use ed25519_dalek::{Signer as _, SigningKey};
use reqwest::{
    Request,
    header::{HeaderValue, InvalidHeaderValue},
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
use thiserror::Error;

const KEYRING_SERVICE: &str = "holonui";
const KEYRING_ACCOUNT: &str = "nobitex-specialized-api-key";

#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct ApiCredentials {
    public_key: String,
    private_key: String,
}

impl ApiCredentials {
    pub(crate) fn new(public_key: impl Into<String>, private_key: impl Into<String>) -> Self {
        Self {
            public_key: public_key.into().trim().to_owned(),
            private_key: private_key.into().trim().to_owned(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ApiKeyAuthError> {
        if self.public_key.is_empty() {
            return Err(ApiKeyAuthError::MissingPublicKey);
        }

        self.signing_key().map(|_| ())?;

        Ok(())
    }

    fn signing_key(&self) -> Result<SigningKey, ApiKeyAuthError> {
        if self.private_key.is_empty() {
            return Err(ApiKeyAuthError::MissingPrivateKey);
        }

        let bytes = URL_SAFE.decode(&self.private_key)?;
        let length = bytes.len();
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| ApiKeyAuthError::InvalidPrivateKeyLength(length))?;

        Ok(SigningKey::from_bytes(&bytes))
    }

    pub(crate) fn authenticate(&self, request: &mut Request) -> Result<(), ApiKeyAuthError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .to_string();

        self.authenticate_at(request, &timestamp)
    }

    fn authenticate_at(
        &self,
        request: &mut Request,
        timestamp: &str,
    ) -> Result<(), ApiKeyAuthError> {
        if self.public_key.is_empty() {
            return Err(ApiKeyAuthError::MissingPublicKey);
        }
        let signing_key = self.signing_key()?;

        let full_path = match request.url().query() {
            Some(query) => format!("{}?{query}", request.url().path()),
            None => request.url().path().to_owned(),
        };
        let raw_body = match request.body() {
            Some(body) => body
                .as_bytes()
                .ok_or(ApiKeyAuthError::RequestBodyNotBuffered)?,
            None => &[],
        };
        let payload = [
            timestamp.as_bytes(),
            request.method().as_str().as_bytes(),
            full_path.as_bytes(),
            raw_body,
        ]
        .concat();
        let signature = signing_key.sign(&payload);
        let signature = URL_SAFE.encode(signature.to_bytes());

        let headers = request.headers_mut();
        headers.insert("Nobitex-Key", HeaderValue::from_str(&self.public_key)?);
        headers.insert("Nobitex-Signature", HeaderValue::from_str(&signature)?);
        headers.insert("Nobitex-Timestamp", HeaderValue::from_str(timestamp)?);

        Ok(())
    }
}

#[derive(Debug, Error)]
pub(crate) enum ApiKeyAuthError {
    #[error("the Nobitex public key is missing")]
    MissingPublicKey,

    #[error("the Nobitex private key is missing")]
    MissingPrivateKey,

    #[error("the Nobitex private key is not URL-safe Base64: {0}")]
    InvalidPrivateKey(#[from] base64::DecodeError),

    #[error("the decoded Nobitex private key is {0} bytes; expected 32")]
    InvalidPrivateKeyLength(usize),

    #[error("the request body is streaming and cannot be signed byte-for-byte")]
    RequestBodyNotBuffered,

    #[error("an API-key header contains an invalid value: {0}")]
    InvalidHeader(#[from] InvalidHeaderValue),

    #[error("the system clock is before the Unix epoch: {0}")]
    InvalidSystemTime(#[from] SystemTimeError),
}

fn credentials_from_env() -> Option<ApiCredentials> {
    let public_key = std::env::var("NOBITEX_PUBLIC_KEY").ok()?;
    let private_key = std::env::var("NOBITEX_PRIVATE_KEY").ok()?;
    Some(ApiCredentials::new(public_key, private_key))
}

pub(crate) fn load_api_credentials() -> ApiCredentials {
    #[cfg(not(target_arch = "wasm32"))]
    {
        log::debug!("Loading Nobitex specialized API key from keyring");
        if let Ok(serialized) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
            .and_then(|entry| entry.get_password())
        {
            match serde_json::from_str(&serialized) {
                Ok(credentials) => return credentials,
                Err(err) => log::error!("Failed to parse Nobitex API key from keyring: {err}"),
            }
        }
    }

    credentials_from_env().unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn save_api_credentials(
    credentials: &ApiCredentials,
) -> Result<(), CredentialStoreError> {
    let serialized = serde_json::to_string(credentials)?;
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)?.set_password(&serialized)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Error)]
pub(crate) enum CredentialStoreError {
    #[error("failed to serialize the API key: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("failed to access the system keyring: {0}")]
    Keyring(#[from] keyring::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_the_exact_method_path_query_and_body() {
        let credentials = ApiCredentials::new(
            "public-key",
            URL_SAFE.encode([
                0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
                0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03,
                0x1c, 0xae, 0x7f, 0x60,
            ]),
        );
        let mut request = reqwest::Client::new()
            .post("https://apiv2.nobitex.ir/example?currency=btc&page=2")
            .body("{\"hours\":2.4}\n")
            .build()
            .unwrap();

        credentials
            .authenticate_at(&mut request, "1700000000")
            .unwrap();

        assert_eq!(request.headers()["Nobitex-Key"], "public-key");
        assert_eq!(request.headers()["Nobitex-Timestamp"], "1700000000");
        assert_eq!(
            request.headers()["Nobitex-Signature"],
            "vGfZ95l-y791PfptxQ2nNYY1AkElXHeJUsjwKm5iNswmdvDrNoxnk9XvC2EBMa1HGFlvxB-iujLna6h0T_K0Dg=="
        );
    }
}
