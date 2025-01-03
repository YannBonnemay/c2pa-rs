// Copyright 2022 Adobe. All rights reserved.
// This file is licensed to you under the Apache License,
// Version 2.0 (http://www.apache.org/licenses/LICENSE-2.0)
// or the MIT license (http://opensource.org/licenses/MIT),
// at your option.

// Unless required by applicable law or agreed to in writing,
// this software is distributed on an "AS IS" BASIS, WITHOUT
// WARRANTIES OR REPRESENTATIONS OF ANY KIND, either express or
// implied. See the LICENSE-MIT and LICENSE-APACHE files for thema
// specific language governing permissions and limitations under
// each license.

#![allow(clippy::unwrap_used)]

#[cfg(feature = "file_io")]
use std::path::Path;
use std::{
    io::{Cursor, Read, Write},
    path::PathBuf,
};

use async_trait::async_trait;
use c2pa_crypto::{
    cose::{CertificateTrustPolicy, TimeStampStorage},
    raw_signature::{AsyncRawSigner, RawSigner, RawSignerError, SigningAlg},
    time_stamp::{AsyncTimeStampProvider, TimeStampError, TimeStampProvider},
};
use tempfile::TempDir;

use crate::{
    assertions::{labels, Action, Actions, Ingredient, ReviewRating, SchemaDotOrg, Thumbnail},
    asset_io::CAIReadWrite,
    claim::Claim,
    hash_utils::Hasher,
    jumbf_io::get_assetio_handler,
    salt::DefaultSalt,
    store::Store,
    AsyncSigner, RemoteSigner, Result,
};

pub const TEST_SMALL_JPEG: &str = "earth_apollo17.jpg";

pub const TEST_WEBP: &str = "mars.webp";

pub const TEST_VC: &str = r#"{
    "@context": [
    "https://www.w3.org/2018/credentials/v1",
    "http://schema.org"
    ],
    "type": [
    "VerifiableCredential",
    "NPPACredential"
    ],
    "issuer": "https://nppa.org/",
    "credentialSubject": {
        "id": "did:nppa:eb1bb9934d9896a374c384521410c7f14",
        "name": "Bob Ross",
        "memberOf": "https://nppa.org/"
    },
    "proof": {
        "type": "RsaSignature2018",
        "created": "2021-06-18T21:19:10Z",
        "proofPurpose": "assertionMethod",
        "verificationMethod":
        "did:nppa:eb1bb9934d9896a374c384521410c7f14#_Qq0UL2Fq651Q0Fjd6TvnYE-faHiOpRlPVQcY_-tA4A",
        "jws": "eyJhbGciOiJQUzI1NiIsImI2NCI6ZmFsc2UsImNyaXQiOlsiYjY0Il19DJBMvvFAIC00nSGB6Tn0XKbbF9XrsaJZREWvR2aONYTQQxnyXirtXnlewJMBBn2h9hfcGZrvnC1b6PgWmukzFJ1IiH1dWgnDIS81BH-IxXnPkbuYDeySorc4QU9MJxdVkY5EL4HYbcIfwKj6X4LBQ2_ZHZIu1jdqLcRZqHcsDF5KKylKc1THn5VRWy5WhYg_gBnyWny8E6Qkrze53MR7OuAmmNJ1m1nN8SxDrG6a08L78J0-Fbas5OjAQz3c17GY8mVuDPOBIOVjMEghBlgl3nOi1ysxbRGhHLEK4s0KKbeRogZdgt1DkQxDFxxn41QWDw_mmMCjs9qxg0zcZzqEJw"
    }
}"#;

/// creates a claim for testing
pub fn create_test_claim() -> Result<Claim> {
    let mut claim = Claim::new("adobe unit test", Some("adobe"));

    // add some data boxes
    let _db_uri = claim.add_databox("text/plain", "this is a test".as_bytes().to_vec(), None)?;
    let _db_uri_1 =
        claim.add_databox("text/plain", "this is more text".as_bytes().to_vec(), None)?;

    // add VC entry
    let _hu = claim.add_verifiable_credential(TEST_VC)?;

    // Add assertions.
    let actions = Actions::new()
        .add_action(
            Action::new("c2pa.cropped")
                .set_parameter(
                    "name".to_owned(),
                    r#"{
                    "left": 0,
                    "right": 2000,
                    "top": 1000,
                    "bottom": 4000
                }"#,
                )
                .unwrap(),
        )
        .add_action(
            Action::new("c2pa.filtered")
                .set_parameter("name".to_owned(), "gaussian blur")?
                .set_when("2015-06-26T16:43:23+0200"),
        );
    // add a binary thumbnail assertion  ('deadbeefadbeadbe')
    let some_binary_data: Vec<u8> = vec![
        0x0d, 0x0e, 0x0a, 0x0d, 0x0b, 0x0e, 0x0e, 0x0f, 0x0a, 0x0d, 0x0b, 0x0e, 0x0a, 0x0d, 0x0b,
        0x0e,
    ];

    // create a schema.org claim
    let cr = r#"{
        "@context": "https://schema.org",
        "@type": "ClaimReview",
        "claimReviewed": "The world is flat",
        "reviewRating": {
            "@type": "Rating",
            "ratingValue": "1",
            "bestRating": "5",
            "worstRating": "1",
            "alternateName": "False"
        }
    }"#;
    let claim_review = SchemaDotOrg::from_json_str(cr)?;

    let thumbnail_claim = Thumbnail::new(labels::JPEG_CLAIM_THUMBNAIL, some_binary_data.clone());

    let thumbnail_ingred = Thumbnail::new(labels::JPEG_INGREDIENT_THUMBNAIL, some_binary_data);

    claim.add_assertion(&actions)?;
    claim.add_assertion(&claim_review)?;
    claim.add_assertion(&thumbnail_claim)?;

    let thumb_uri = claim.add_assertion_with_salt(&thumbnail_ingred, &DefaultSalt::default())?;

    let review = ReviewRating::new(
        "a 3rd party plugin was used",
        Some("actions.unknownActionsPerformed".to_string()),
        1,
    );

    //let data_path = claim.add_ingredient_data("some data".as_bytes());
    let ingredient = Ingredient::new(
        "image 1.jpg",
        "image/jpeg",
        "xmp.iid:7b57930e-2f23-47fc-affe-0400d70b738d",
        Some("xmp.did:87d51599-286e-43b2-9478-88c79f49c347"),
    )
    .set_thumbnail(Some(&thumb_uri))
    //.set_manifest_data(&data_path)
    .add_review(review);

    let ingredient2 = Ingredient::new(
        "image 2.png",
        "image/png",
        "xmp.iid:7b57930e-2f23-47fc-affe-0400d70b738c",
        Some("xmp.did:87d51599-286e-43b2-9478-88c79f49c346"),
    )
    .set_thumbnail(Some(&thumb_uri));

    claim.add_assertion_with_salt(&ingredient, &DefaultSalt::default())?;
    claim.add_assertion_with_salt(&ingredient2, &DefaultSalt::default())?;

    Ok(claim)
}

/// Creates a store with an unsigned claim for testing
pub fn create_test_store() -> Result<Store> {
    // Create claims store.
    let mut store = Store::new();

    let claim = create_test_claim()?;
    store.commit_claim(claim).unwrap();
    Ok(store)
}

/// returns a path to a file in the fixtures folder
pub fn fixture_path(file_name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(file_name);
    path
}

/// returns a path to a file in the temp_dir folder
// note, you must pass TempDir from the caller's context
pub fn temp_dir_path(temp_dir: &TempDir, file_name: &str) -> PathBuf {
    let mut path = PathBuf::from(temp_dir.path());
    path.push(file_name);
    path
}

// copies a fixture to a temp file and returns path to copy
pub fn temp_fixture_path(temp_dir: &TempDir, file_name: &str) -> PathBuf {
    let fixture_src = fixture_path(file_name);
    let fixture_copy = temp_dir_path(temp_dir, file_name);
    std::fs::copy(fixture_src, &fixture_copy).unwrap();
    fixture_copy
}

/// Create a [`Signer`] instance that can be used for testing purposes.
///
/// This is a suitable default for use when you need a [`Signer`], but
/// don't care what the format is.
///
/// # Returns
///
/// Returns a boxed [`Signer`] instance.
///
/// # Panics
///
/// Can panic if the certs cannot be read. (This function should only
/// be used as part of testing infrastructure.)
#[cfg(feature = "file_io")]
pub fn temp_signer_file() -> Box<dyn crate::Signer> {
    #![allow(clippy::expect_used)]
    let mut sign_cert_path = fixture_path("certs");
    sign_cert_path.push("ps256");
    sign_cert_path.set_extension("pub");

    let mut pem_key_path = fixture_path("certs");
    pem_key_path.push("ps256");
    pem_key_path.set_extension("pem");

    crate::create_signer::from_files(&sign_cert_path, &pem_key_path, SigningAlg::Ps256, None)
        .expect("get_temp_signer")
}

/// Create a [`CertificateTrustPolicy`] instance that has the test certificate bundles included.
///
/// [`CertificateTrustPolicy`]: c2pa_crypto::cose::CertificateTrustPolicy
pub fn test_certificate_acceptance_policy() -> CertificateTrustPolicy {
    let mut ctp = CertificateTrustPolicy::default();
    ctp.add_trust_anchors(include_bytes!(
        "../../tests/fixtures/certs/trust/test_cert_root_bundle.pem"
    ))
    .unwrap();
    ctp
}

#[cfg(feature = "file_io")]
pub fn write_jpeg_placeholder_file(
    placeholder: &[u8],
    input: &Path,
    output_file: &mut dyn CAIReadWrite,
    hasher: Option<&mut Hasher>,
) -> Result<usize> {
    let mut f = std::fs::File::open(input).unwrap();
    write_jpeg_placeholder_stream(placeholder, "jpeg", &mut f, output_file, hasher)
}

/// Utility to create a test file with a placeholder for a manifest
pub fn write_jpeg_placeholder_stream<R>(
    placeholder: &[u8],
    format: &str,
    input: &mut R,
    output_file: &mut dyn CAIReadWrite,
    mut hasher: Option<&mut Hasher>,
) -> Result<usize>
where
    R: Read + std::io::Seek + Send,
{
    let jpeg_io = get_assetio_handler(format).unwrap();
    let box_mapper = jpeg_io.asset_box_hash_ref().unwrap();
    let boxes = box_mapper.get_box_map(input).unwrap();
    let sof = boxes.iter().find(|b| b.names[0] == "SOF0").unwrap();

    // build new asset with hole for new manifest
    let outbuf = Vec::new();
    let mut out_stream = Cursor::new(outbuf);
    input.rewind().unwrap();

    // write before
    let mut before = vec![0u8; sof.range_start];
    input.read_exact(before.as_mut_slice()).unwrap();
    if let Some(hasher) = hasher.as_deref_mut() {
        hasher.update(&before);
    }
    out_stream.write_all(&before).unwrap();

    // write placeholder
    out_stream.write_all(placeholder).unwrap();

    // write bytes after
    let mut after_buf = Vec::new();
    input.read_to_end(&mut after_buf).unwrap();
    if let Some(hasher) = hasher {
        hasher.update(&after_buf);
    }
    out_stream.write_all(&after_buf).unwrap();

    // save to output file
    output_file.write_all(&out_stream.into_inner()).unwrap();

    Ok(sof.range_start)
}

pub(crate) struct TestGoodSigner {}

impl crate::Signer for TestGoodSigner {
    fn sign(&self, _data: &[u8]) -> Result<Vec<u8>> {
        Ok(b"not a valid signature".to_vec())
    }

    fn alg(&self) -> SigningAlg {
        SigningAlg::Ps256
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        Ok(Vec::new())
    }

    fn reserve_size(&self) -> usize {
        1024
    }

    fn send_timestamp_request(&self, _message: &[u8]) -> Option<crate::error::Result<Vec<u8>>> {
        Some(Ok(Vec::new()))
    }

    fn raw_signer(&self) -> Box<&dyn RawSigner> {
        Box::new(self)
    }
}

impl RawSigner for TestGoodSigner {
    fn sign(&self, _data: &[u8]) -> std::result::Result<Vec<u8>, RawSignerError> {
        Ok(b"not a valid signature".to_vec())
    }

    fn alg(&self) -> SigningAlg {
        SigningAlg::Ps256
    }

    fn cert_chain(&self) -> std::result::Result<Vec<Vec<u8>>, RawSignerError> {
        Ok(Vec::new())
    }

    fn reserve_size(&self) -> usize {
        1024
    }
}

impl TimeStampProvider for TestGoodSigner {
    fn send_time_stamp_request(
        &self,
        _message: &[u8],
    ) -> Option<std::result::Result<Vec<u8>, TimeStampError>> {
        Some(Ok(Vec::new()))
    }
}

pub(crate) struct AsyncTestGoodSigner {}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AsyncSigner for AsyncTestGoodSigner {
    async fn sign(&self, _data: Vec<u8>) -> Result<Vec<u8>> {
        Ok(b"not a valid signature".to_vec())
    }

    fn alg(&self) -> SigningAlg {
        SigningAlg::Ps256
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        Ok(Vec::new())
    }

    fn reserve_size(&self) -> usize {
        1024
    }

    async fn send_timestamp_request(
        &self,
        _message: &[u8],
    ) -> Option<crate::error::Result<Vec<u8>>> {
        Some(Ok(Vec::new()))
    }

    fn async_raw_signer(&self) -> Box<&dyn AsyncRawSigner> {
        Box::new(self)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AsyncRawSigner for AsyncTestGoodSigner {
    async fn sign(&self, _data: Vec<u8>) -> std::result::Result<Vec<u8>, RawSignerError> {
        Ok(b"not a valid signature".to_vec())
    }

    fn alg(&self) -> SigningAlg {
        SigningAlg::Ps256
    }

    fn cert_chain(&self) -> std::result::Result<Vec<Vec<u8>>, RawSignerError> {
        Ok(Vec::new())
    }

    fn reserve_size(&self) -> usize {
        1024
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AsyncTimeStampProvider for AsyncTestGoodSigner {
    async fn send_time_stamp_request(
        &self,
        _message: &[u8],
    ) -> Option<std::result::Result<Vec<u8>, TimeStampError>> {
        Some(Ok(Vec::new()))
    }
}

struct TempRemoteSigner {}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl crate::signer::RemoteSigner for TempRemoteSigner {
    async fn sign_remote(&self, claim_bytes: &[u8]) -> crate::error::Result<Vec<u8>> {
        #[cfg(all(feature = "openssl_sign", feature = "file_io"))]
        {
            let signer = crate::utils::test_signer::async_test_signer(SigningAlg::Ps256);

            // this would happen on some remote server
            // TEMPORARY: Assume v1 until we plumb things through further.
            crate::cose_sign::cose_sign_async(
                &signer,
                claim_bytes,
                self.reserve_size(),
                TimeStampStorage::V1_sigTst,
            )
            .await
        }
        #[cfg(not(any(
            target_arch = "wasm32",
            all(feature = "openssl_sign", feature = "file_io")
        )))]
        {
            use std::io::{Seek, Write};

            let mut sign_bytes = std::io::Cursor::new(vec![0u8; self.reserve_size()]);

            sign_bytes.rewind()?;
            sign_bytes.write_all(claim_bytes)?;

            // fake sig
            Ok(sign_bytes.into_inner())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let signer = crate::wasm::RsaWasmSignerAsync::new();

            // TEMPORARY: Assume V1 until we plumb more through.
            crate::cose_sign::cose_sign_async(
                &signer,
                claim_bytes,
                self.reserve_size(),
                TimeStampStorage::V1_sigTst,
            )
            .await
        }
    }

    fn reserve_size(&self) -> usize {
        10000
    }
}

#[cfg(target_arch = "wasm32")]
struct WebCryptoSigner {
    signing_alg: SigningAlg,
    signing_alg_name: String,
    certs: Vec<Vec<u8>>,
    key: Vec<u8>,
}

#[cfg(target_arch = "wasm32")]
impl WebCryptoSigner {
    pub fn new(alg: &str, cert: &str, key: &str) -> Self {
        static START_CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----";
        static END_CERTIFICATE: &str = "-----END CERTIFICATE-----";
        static START_KEY: &str = "-----BEGIN PRIVATE KEY-----";
        static END_KEY: &str = "-----END PRIVATE KEY-----";

        let mut name = alg.to_owned().to_uppercase();
        name.insert(2, '-');

        let key = key
            .replace("\n", "")
            .replace(START_KEY, "")
            .replace(END_KEY, "");
        let key = c2pa_crypto::base64::decode(&key).unwrap();

        let certs = cert
            .replace("\n", "")
            .replace(START_CERTIFICATE, "")
            .split(END_CERTIFICATE)
            .map(|x| c2pa_crypto::base64::decode(x).unwrap())
            .collect();

        Self {
            signing_alg: alg.parse().unwrap(),
            signing_alg_name: name,
            certs,
            key,
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait::async_trait(?Send)]
impl AsyncSigner for WebCryptoSigner {
    fn alg(&self) -> SigningAlg {
        self.signing_alg
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        Ok(self.certs.clone())
    }

    async fn sign(&self, claim_bytes: Vec<u8>) -> crate::error::Result<Vec<u8>> {
        use c2pa_crypto::webcrypto::WindowOrWorker;
        use js_sys::{Array, Object, Reflect, Uint8Array};
        use wasm_bindgen_futures::JsFuture;
        use web_sys::CryptoKey;
        let context = WindowOrWorker::new().unwrap();
        let crypto = context.subtle_crypto().unwrap();

        let mut data = claim_bytes.clone();
        let promise = crypto
            .digest_with_str_and_u8_array("SHA-256", &mut data)
            .unwrap();
        let result = JsFuture::from(promise).await.unwrap();
        let mut digest = Uint8Array::new(&result).to_vec();

        let key = Uint8Array::new_with_length(self.key.len() as u32);
        key.copy_from(&self.key);
        let usages = Array::new();
        usages.push(&"sign".into());
        let alg = Object::new();
        Reflect::set(&alg, &"name".into(), &"ECDSA".into()).unwrap();
        Reflect::set(&alg, &"namedCurve".into(), &"P-256".into()).unwrap();

        let promise = crypto
            .import_key_with_object("pkcs8", &key, &alg, true, &usages)
            .unwrap();
        let key: CryptoKey = JsFuture::from(promise).await.unwrap().into();

        let alg = Object::new();
        Reflect::set(&alg, &"name".into(), &"ECDSA".into()).unwrap();
        Reflect::set(&alg, &"hash".into(), &"SHA-256".into()).unwrap();
        let promise = crypto
            .sign_with_object_and_u8_array(&alg, &key, &mut digest)
            .unwrap();
        let result = JsFuture::from(promise).await.unwrap();
        Ok(Uint8Array::new(&result).to_vec())
    }

    fn reserve_size(&self) -> usize {
        10000
    }

    async fn send_timestamp_request(&self, _: &[u8]) -> Option<Result<Vec<u8>>> {
        None
    }

    fn async_raw_signer(&self) -> Box<&dyn AsyncRawSigner> {
        unreachable!();
    }
}

/// Create a [`RemoteSigner`] instance that can be used for testing purposes.
///
/// # Returns
///
/// Returns a boxed [`RemoteSigner`] instance.
pub fn temp_remote_signer() -> Box<dyn RemoteSigner> {
    Box::new(TempRemoteSigner {})
}

/// Create an AsyncSigner that acts as a RemoteSigner
struct TempAsyncRemoteSigner {
    signer: TempRemoteSigner,
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl AsyncSigner for TempAsyncRemoteSigner {
    // this will not be called but requires an implementation
    async fn sign(&self, claim_bytes: Vec<u8>) -> Result<Vec<u8>> {
        #[cfg(all(feature = "openssl_sign", feature = "file_io"))]
        {
            let signer = crate::utils::test_signer::async_test_signer(SigningAlg::Ps256);

            // this would happen on some remote server
            // TEMPORARY: Assume V1 until we plumb through further.
            crate::cose_sign::cose_sign_async(
                &signer,
                &claim_bytes,
                AsyncSigner::reserve_size(self),
                TimeStampStorage::V1_sigTst,
            )
            .await
        }

        #[cfg(target_arch = "wasm32")]
        {
            let signer = crate::wasm::rsa_wasm_signer::RsaWasmSignerAsync::new();
            // TEMPORARY: Assume V1 until we plumb through further.
            crate::cose_sign::cose_sign_async(
                &signer,
                &claim_bytes,
                AsyncRawSigner::reserve_size(self),
                TimeStampStorage::V1_sigTst,
            )
            .await
        }

        #[cfg(not(any(
            target_arch = "wasm32",
            all(feature = "openssl_sign", feature = "file_io")
        )))]
        {
            use std::io::{Seek, Write};

            let mut sign_bytes = std::io::Cursor::new(vec![0u8; self.reserve_size()]);

            sign_bytes.rewind()?;
            sign_bytes.write_all(&claim_bytes)?;

            // fake sig
            Ok(sign_bytes.into_inner())
        }
    }

    // signer will return a COSE structure
    fn direct_cose_handling(&self) -> bool {
        true
    }

    fn alg(&self) -> SigningAlg {
        SigningAlg::Ps256
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        Ok(Vec::new())
    }

    fn reserve_size(&self) -> usize {
        10000
    }

    async fn send_timestamp_request(
        &self,
        _message: &[u8],
    ) -> Option<crate::error::Result<Vec<u8>>> {
        Some(Ok(Vec::new()))
    }

    fn async_raw_signer(&self) -> Box<&dyn AsyncRawSigner> {
        Box::new(self)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl AsyncRawSigner for TempAsyncRemoteSigner {
    async fn sign(&self, _claim_bytes: Vec<u8>) -> std::result::Result<Vec<u8>, RawSignerError> {
        unreachable!("Should not be called");
    }

    fn alg(&self) -> SigningAlg {
        SigningAlg::Ps256
    }

    fn cert_chain(&self) -> std::result::Result<Vec<Vec<u8>>, RawSignerError> {
        Ok(Vec::new())
    }

    fn reserve_size(&self) -> usize {
        10000
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl AsyncTimeStampProvider for TempAsyncRemoteSigner {
    async fn send_time_stamp_request(
        &self,
        _message: &[u8],
    ) -> Option<std::result::Result<Vec<u8>, TimeStampError>> {
        Some(Ok(Vec::new()))
    }
}

/// Create a [`AsyncSigner`] that does it's own COSE handling for testing.
///
/// # Returns
///
/// Returns a boxed [`RemoteSigner`] instance.
pub fn temp_async_remote_signer() -> Box<dyn crate::signer::AsyncSigner> {
    Box::new(TempAsyncRemoteSigner {
        signer: TempRemoteSigner {},
    })
}

#[test]
fn test_create_test_store() {
    #[allow(clippy::expect_used)]
    let store = create_test_store().expect("create test store");

    assert_eq!(store.claims().len(), 1);
}
