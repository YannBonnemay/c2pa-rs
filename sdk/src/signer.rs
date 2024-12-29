// Copyright 2022 Adobe. All rights reserved.
// This file is licensed to you under the Apache License,
// Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
// or the MIT license (http://opensource.org/licenses/MIT),
// at your option.

// Unless required by applicable law or agreed to in writing,
// this software is distributed on an "AS IS" BASIS, WITHOUT
// WARRANTIES OR REPRESENTATIONS OF ANY KIND, either express or
// implied. See the LICENSE-MIT and LICENSE-APACHE files for the
// specific language governing permissions and limitations under
// each license.

use async_trait::async_trait;
use c2pa_crypto::{
    raw_signature::RawSigner,
    time_stamp::{AsyncTimeStampProvider, TimeStampProvider},
    SigningAlg,
};

use crate::{DynamicAssertion, Result};

/// The `Signer` trait generates a cryptographic signature over a byte array.
///
/// This trait exists to allow the signature mechanism to be extended.
pub trait Signer {
    /// Returns a new byte array which is a signature over the original.
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Returns the algorithm of the Signer.
    fn alg(&self) -> SigningAlg;

    /// Returns the certificates as a Vec containing a Vec of DER bytes for each certificate.
    fn certs(&self) -> Result<Vec<Vec<u8>>>;

    /// Returns the size in bytes of the largest possible expected signature.
    /// Signing will fail if the result of the `sign` function is larger
    /// than this value.
    fn reserve_size(&self) -> usize;

    /// URL for time authority to time stamp the signature
    fn time_authority_url(&self) -> Option<String> {
        None
    }

    /// Additional request headers to pass to the time stamp authority.
    ///
    /// IMPORTANT: You should not include the "Content-type" header here.
    /// That is provided by default.
    fn timestamp_request_headers(&self) -> Option<Vec<(String, String)>> {
        None
    }

    fn timestamp_request_body(&self, message: &[u8]) -> Result<Vec<u8>> {
        c2pa_crypto::time_stamp::default_rfc3161_message(message).map_err(|e| e.into())
    }

    /// Request RFC 3161 timestamp to be included in the manifest data
    /// structure.
    ///
    /// `message` is a preliminary hash of the claim
    ///
    /// The default implementation will send the request to the URL
    /// provided by [`Self::time_authority_url()`], if any.
    #[allow(unused)] // message not used on WASM
    fn send_timestamp_request(&self, message: &[u8]) -> Option<Result<Vec<u8>>> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(url) = self.time_authority_url() {
            if let Ok(body) = self.timestamp_request_body(message) {
                let headers: Option<Vec<(String, String)>> = self.timestamp_request_headers();
                return Some(
                    c2pa_crypto::time_stamp::default_rfc3161_request(&url, headers, &body, message)
                        .map_err(|e| e.into()),
                );
            }
        }

        None
    }

    /// OCSP response for the signing cert if available
    /// This is the only C2PA supported cert revocation method.
    /// By pre-querying the value for a your signing cert the value can
    /// be cached taking pressure off of the CA (recommended by C2PA spec)
    fn ocsp_val(&self) -> Option<Vec<u8>> {
        None
    }

    /// If this returns true the sign function is responsible for for direct handling of the COSE structure.
    ///
    /// This is useful for cases where the signer needs to handle the COSE structure directly.
    /// Not recommended for general use.
    fn direct_cose_handling(&self) -> bool {
        false
    }

    /// Returns a list of dynamic assertions that should be included in the manifest.
    fn dynamic_assertions(&self) -> Vec<Box<dyn DynamicAssertion>> {
        Vec::new()
    }

    /// If this struct also implements [`TimeStampProvider`], return a reference to that struct.
    ///
    /// [`TimeStampProvider`]: c2pa_crypto::time_stamp::TimeStampProvider
    fn time_stamp_provider(&self) -> Option<Box<&dyn TimeStampProvider>> {
        None
    }
}

/// Trait to allow loading of signing credential from external sources
#[allow(dead_code)] // this here for wasm builds to pass clippy  (todo: remove)
pub(crate) trait ConfigurableSigner: Signer + Sized {
    /// Create signer form credential files
    #[cfg(feature = "file_io")]
    fn from_files<P: AsRef<std::path::Path>>(
        signcert_path: P,
        pkey_path: P,
        alg: SigningAlg,
        tsa_url: Option<String>,
    ) -> Result<Self> {
        let signcert = std::fs::read(signcert_path).map_err(crate::Error::IoError)?;
        let pkey = std::fs::read(pkey_path).map_err(crate::Error::IoError)?;

        Self::from_signcert_and_pkey(&signcert, &pkey, alg, tsa_url)
    }

    /// Create signer from credentials data
    fn from_signcert_and_pkey(
        signcert: &[u8],
        pkey: &[u8],
        alg: SigningAlg,
        tsa_url: Option<String>,
    ) -> Result<Self>;
}

/// The `AsyncSigner` trait generates a cryptographic signature over a byte array.
///
/// This trait exists to allow the signature mechanism to be extended.
///
/// Use this when the implementation is asynchronous.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait AsyncSigner: Sync {
    /// Returns a new byte array which is a signature over the original.
    async fn sign(&self, data: Vec<u8>) -> Result<Vec<u8>>;

    /// Returns the algorithm of the Signer.
    fn alg(&self) -> SigningAlg;

    /// Returns the certificates as a Vec containing a Vec of DER bytes for each certificate.
    fn certs(&self) -> Result<Vec<Vec<u8>>>;

    /// Returns the size in bytes of the largest possible expected signature.
    /// Signing will fail if the result of the `sign` function is larger
    /// than this value.
    fn reserve_size(&self) -> usize;

    /// URL for time authority to time stamp the signature
    fn time_authority_url(&self) -> Option<String> {
        None
    }

    /// Additional request headers to pass to the time stamp authority.
    ///
    /// IMPORTANT: You should not include the "Content-type" header here.
    /// That is provided by default.
    fn timestamp_request_headers(&self) -> Option<Vec<(String, String)>> {
        None
    }

    fn timestamp_request_body(&self, message: &[u8]) -> Result<Vec<u8>> {
        c2pa_crypto::time_stamp::default_rfc3161_message(message).map_err(|e| e.into())
    }

    /// Request RFC 3161 timestamp to be included in the manifest data
    /// structure.
    ///
    /// `message` is a preliminary hash of the claim
    ///
    /// The default implementation will send the request to the URL
    /// provided by [`Self::time_authority_url()`], if any.
    async fn send_timestamp_request(&self, message: &[u8]) -> Option<Result<Vec<u8>>> {
        // NOTE: This is currently synchronous, but may become
        // async in the future.
        if let Some(url) = self.time_authority_url() {
            if let Ok(body) = self.timestamp_request_body(message) {
                let headers: Option<Vec<(String, String)>> = self.timestamp_request_headers();
                return Some(
                    c2pa_crypto::time_stamp::default_rfc3161_request_async(
                        &url, headers, &body, message,
                    )
                    .await
                    .map_err(|e| e.into()),
                );
            }
        }

        None
    }

    /// OCSP response for the signing cert if available
    /// This is the only C2PA supported cert revocation method.
    /// By pre-querying the value for a your signing cert the value can
    /// be cached taking pressure off of the CA (recommended by C2PA spec)
    async fn ocsp_val(&self) -> Option<Vec<u8>> {
        None
    }

    /// If this returns true the sign function is responsible for for direct handling of the COSE structure.
    ///
    /// This is useful for cases where the signer needs to handle the COSE structure directly.
    /// Not recommended for general use.
    fn direct_cose_handling(&self) -> bool {
        false
    }

    /// Returns a list of dynamic assertions that should be included in the manifest.
    fn dynamic_assertions(&self) -> Vec<Box<dyn DynamicAssertion>> {
        Vec::new()
    }

    /// If this struct also implements [`AsyncTimeStampProvider`], return a reference to that struct.
    ///
    /// [`AsyncTimeStampProvider`]: c2pa_crypto::time_stamp::AsyncTimeStampProvider
    fn async_time_stamp_provider(&self) -> Option<Box<&dyn AsyncTimeStampProvider>> {
        None
    }
}

/// The `AsyncSigner` trait generates a cryptographic signature over a byte array.
///
/// This trait exists to allow the signature mechanism to be extended.
///
/// Use this when the implementation is asynchronous.
#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait AsyncSigner {
    /// Returns a new byte array which is a signature over the original.
    async fn sign(&self, data: Vec<u8>) -> Result<Vec<u8>>;

    /// Returns the algorithm of the Signer.
    fn alg(&self) -> SigningAlg;

    /// Returns the certificates as a Vec containing a Vec of DER bytes for each certificate.
    fn certs(&self) -> Result<Vec<Vec<u8>>>;

    /// Returns the size in bytes of the largest possible expected signature.
    /// Signing will fail if the result of the `sign` function is larger
    /// than this value.
    fn reserve_size(&self) -> usize;

    /// URL for time authority to time stamp the signature
    fn time_authority_url(&self) -> Option<String> {
        None
    }

    /// Additional request headers to pass to the time stamp authority.
    ///
    /// IMPORTANT: You should not include the "Content-type" header here.
    /// That is provided by default.
    fn timestamp_request_headers(&self) -> Option<Vec<(String, String)>> {
        None
    }

    fn timestamp_request_body(&self, message: &[u8]) -> Result<Vec<u8>> {
        c2pa_crypto::time_stamp::default_rfc3161_message(message).map_err(|e| e.into())
    }

    /// Request RFC 3161 timestamp to be included in the manifest data
    /// structure.
    ///
    /// `message` is a preliminary hash of the claim
    ///
    /// The default implementation will send the request to the URL
    /// provided by [`Self::time_authority_url()`], if any.
    async fn send_timestamp_request(&self, _message: &[u8]) -> Option<Result<Vec<u8>>> {
        None
    }

    /// OCSP response for the signing cert if available
    /// This is the only C2PA supported cert revocation method.
    /// By pre-querying the value for a your signing cert the value can
    /// be cached taking pressure off of the CA (recommended by C2PA spec)
    async fn ocsp_val(&self) -> Option<Vec<u8>> {
        None
    }

    /// If this returns true the sign function is responsible for for direct handling of the COSE structure.
    ///
    /// This is useful for cases where the signer needs to handle the COSE structure directly.
    /// Not recommended for general use.
    fn direct_cose_handling(&self) -> bool {
        false
    }

    /// Returns a list of dynamic assertions that should be included in the manifest.
    fn dynamic_assertions(&self) -> Vec<Box<dyn DynamicAssertion>> {
        Vec::new()
    }

    /// If this struct also implements [`AsyncTimeStampProvider`], return a reference to that struct.
    ///
    /// [`AsyncTimeStampProvider`]: c2pa_crypto::time_stamp::AsyncTimeStampProvider
    fn async_time_stamp_provider<'a>(&'a self) -> Option<Box<&'a dyn AsyncTimeStampProvider>> {
        None
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait RemoteSigner: Sync {
    /// Returns the `CoseSign1` bytes signed by the [`RemoteSigner`].
    ///
    /// The size of returned `Vec` must match the value returned by `reserve_size`.
    /// This data will be embedded in the JUMBF `c2pa.signature` box of the manifest.
    /// `data` are the bytes of the claim to be remotely signed.
    async fn sign_remote(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Returns the size in bytes of the largest possible expected signature.
    ///
    /// Signing will fail if the result of the `sign` function is larger
    /// than this value.
    fn reserve_size(&self) -> usize;
}

impl Signer for Box<dyn Signer> {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        (**self).sign(data)
    }

    fn alg(&self) -> SigningAlg {
        (**self).alg()
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        (**self).certs()
    }

    fn reserve_size(&self) -> usize {
        (**self).reserve_size()
    }

    fn ocsp_val(&self) -> Option<Vec<u8>> {
        (**self).ocsp_val()
    }

    fn direct_cose_handling(&self) -> bool {
        (**self).direct_cose_handling()
    }

    fn dynamic_assertions(&self) -> Vec<Box<dyn DynamicAssertion>> {
        (**self).dynamic_assertions()
    }

    fn time_authority_url(&self) -> Option<String> {
        (**self).time_authority_url()
    }

    fn timestamp_request_headers(&self) -> Option<Vec<(String, String)>> {
        (**self).timestamp_request_headers()
    }

    fn timestamp_request_body(&self, message: &[u8]) -> Result<Vec<u8>> {
        (**self).timestamp_request_body(message)
    }

    fn send_timestamp_request(&self, message: &[u8]) -> Option<Result<Vec<u8>>> {
        (**self).send_timestamp_request(message)
    }

    fn time_stamp_provider(&self) -> Option<Box<&dyn TimeStampProvider>> {
        (**self).time_stamp_provider()
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl AsyncSigner for Box<dyn AsyncSigner + Send + Sync> {
    async fn sign(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        (**self).sign(data).await
    }

    fn alg(&self) -> SigningAlg {
        (**self).alg()
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        (**self).certs()
    }

    fn reserve_size(&self) -> usize {
        (**self).reserve_size()
    }

    fn time_authority_url(&self) -> Option<String> {
        (**self).time_authority_url()
    }

    fn timestamp_request_headers(&self) -> Option<Vec<(String, String)>> {
        (**self).timestamp_request_headers()
    }

    fn timestamp_request_body(&self, message: &[u8]) -> Result<Vec<u8>> {
        (**self).timestamp_request_body(message)
    }

    async fn send_timestamp_request(&self, message: &[u8]) -> Option<Result<Vec<u8>>> {
        (**self).send_timestamp_request(message).await
    }

    async fn ocsp_val(&self) -> Option<Vec<u8>> {
        (**self).ocsp_val().await
    }

    fn direct_cose_handling(&self) -> bool {
        (**self).direct_cose_handling()
    }

    fn dynamic_assertions(&self) -> Vec<Box<dyn DynamicAssertion>> {
        (**self).dynamic_assertions()
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl AsyncSigner for Box<dyn AsyncSigner> {
    async fn sign(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        (**self).sign(data).await
    }

    fn alg(&self) -> SigningAlg {
        (**self).alg()
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        (**self).certs()
    }

    fn reserve_size(&self) -> usize {
        (**self).reserve_size()
    }

    fn time_authority_url(&self) -> Option<String> {
        (**self).time_authority_url()
    }

    fn timestamp_request_headers(&self) -> Option<Vec<(String, String)>> {
        (**self).timestamp_request_headers()
    }

    fn timestamp_request_body(&self, message: &[u8]) -> Result<Vec<u8>> {
        (**self).timestamp_request_body(message)
    }

    async fn send_timestamp_request(&self, message: &[u8]) -> Option<Result<Vec<u8>>> {
        (**self).send_timestamp_request(message).await
    }

    async fn ocsp_val(&self) -> Option<Vec<u8>> {
        (**self).ocsp_val().await
    }

    fn direct_cose_handling(&self) -> bool {
        (**self).direct_cose_handling()
    }

    fn dynamic_assertions(&self) -> Vec<Box<dyn DynamicAssertion>> {
        (**self).dynamic_assertions()
    }
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct RawSignerWrapper(pub(crate) Box<dyn RawSigner>);

impl Signer for RawSignerWrapper {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.0.sign(data).map_err(|e| e.into())
    }

    fn alg(&self) -> SigningAlg {
        self.0.alg()
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        self.0.cert_chain().map_err(|e| e.into())
    }

    fn reserve_size(&self) -> usize {
        self.0.reserve_size()
    }

    fn ocsp_val(&self) -> Option<Vec<u8>> {
        self.0.ocsp_response()
    }

    fn time_authority_url(&self) -> Option<String> {
        self.0.time_stamp_service_url()
    }

    fn timestamp_request_headers(&self) -> Option<Vec<(String, String)>> {
        self.0.time_stamp_request_headers()
    }

    fn timestamp_request_body(&self, message: &[u8]) -> Result<Vec<u8>> {
        self.0
            .time_stamp_request_body(message)
            .map_err(|e| e.into())
    }

    fn send_timestamp_request(&self, message: &[u8]) -> Option<Result<Vec<u8>>> {
        self.0
            .send_time_stamp_request(message)
            .map(|r| r.map_err(|e| e.into()))
    }
}
