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

use crate::{
    ocsp_utils::{get_ocsp_response, OcspData},
    signer::ConfigurableSigner,
    Error, Result, Signer,
};
use std::{cell::Cell, fs, path::Path};

//use extfmt::Hexlify;
use openssl::{
    hash::MessageDigest,
    pkey::{PKey, Private},
    rsa::Rsa,
    x509::X509,
};

use super::check_chain_order;

/// Implements `Signer` trait using OpenSSL's implementation of
/// SHA256 + RSA encryption.
pub struct RsaSigner {
    signcerts: Vec<X509>,
    pkey: PKey<Private>,

    certs_size: usize,
    timestamp_size: usize,
    ocsp_size: Cell<usize>,

    alg: String,
    tsa_url: Option<String>,
    ocsp_rsp: Cell<OcspData>,
}

impl RsaSigner {
    pub fn update_ocsp(&self) {
        // do we need an update
        let now = chrono::offset::Utc::now();

        // is it time for an OCSP update
        let ocsp_data = self.ocsp_rsp.take();
        let next_update = ocsp_data.next_update;
        self.ocsp_rsp.set(ocsp_data);
        if now < next_update {
            return;
        }

        if let Ok(certs) = self.certs() {
            if let Some(ocsp_rsp) = get_ocsp_response(&certs) {
                self.ocsp_size.set(ocsp_rsp.ocsp_der.len());
                self.ocsp_rsp.set(ocsp_rsp);
            }
        }
    }
}

impl ConfigurableSigner for RsaSigner {
    fn from_files<P: AsRef<Path>>(
        signcert_path: P,
        pkey_path: P,
        alg: String,
        tsa_url: Option<String>,
    ) -> Result<Self> {
        let signcert = fs::read(signcert_path).map_err(wrap_io_err)?;
        let pkey = fs::read(pkey_path).map_err(wrap_io_err)?;

        Self::from_signcert_and_pkey(&signcert, &pkey, alg, tsa_url)
    }

    fn from_signcert_and_pkey(
        signcert: &[u8],
        pkey: &[u8],
        alg: String,
        tsa_url: Option<String>,
    ) -> Result<Self> {
        let signcerts = X509::stack_from_pem(signcert).map_err(wrap_openssl_err)?;
        let rsa = Rsa::private_key_from_pem(pkey).map_err(wrap_openssl_err)?;
        let pkey = PKey::from_rsa(rsa).map_err(wrap_openssl_err)?;

        // make sure cert chains are in order
        if !check_chain_order(&signcerts) {
            return Err(Error::BadParam(
                "certificate chain is not in correct order".to_string(),
            ));
        }

        let signer = RsaSigner {
            signcerts,
            pkey,
            certs_size: signcert.len(),
            timestamp_size: 4096, // todo: call out to TSA to get actual timestamp and use that size
            ocsp_size: Cell::new(0),
            alg,
            tsa_url,
            ocsp_rsp: Cell::new(OcspData::new()),
        };

        // get OCSP if possible
        signer.update_ocsp();

        Ok(signer)
    }
}

impl Signer for RsaSigner {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut signer = match self.alg.as_str() {
            "ps256" => {
                let mut signer = openssl::sign::Signer::new(MessageDigest::sha256(), &self.pkey)
                    .map_err(wrap_openssl_err)?;

                signer.set_rsa_padding(openssl::rsa::Padding::PKCS1_PSS)?; // use C2PA recommended padding
                signer.set_rsa_mgf1_md(MessageDigest::sha256())?;
                signer.set_rsa_pss_saltlen(openssl::sign::RsaPssSaltlen::DIGEST_LENGTH)?;
                signer
            }
            "ps384" => {
                let mut signer = openssl::sign::Signer::new(MessageDigest::sha384(), &self.pkey)
                    .map_err(wrap_openssl_err)?;

                signer.set_rsa_padding(openssl::rsa::Padding::PKCS1_PSS)?; // use C2PA recommended padding
                signer.set_rsa_mgf1_md(MessageDigest::sha384())?;
                signer.set_rsa_pss_saltlen(openssl::sign::RsaPssSaltlen::DIGEST_LENGTH)?;
                signer
            }
            "ps512" => {
                let mut signer = openssl::sign::Signer::new(MessageDigest::sha512(), &self.pkey)
                    .map_err(wrap_openssl_err)?;

                signer.set_rsa_padding(openssl::rsa::Padding::PKCS1_PSS)?; // use C2PA recommended padding
                signer.set_rsa_mgf1_md(MessageDigest::sha512())?;
                signer.set_rsa_pss_saltlen(openssl::sign::RsaPssSaltlen::DIGEST_LENGTH)?;
                signer
            }
            "rs256" => openssl::sign::Signer::new(MessageDigest::sha256(), &self.pkey)
                .map_err(wrap_openssl_err)?,
            "rs384" => openssl::sign::Signer::new(MessageDigest::sha384(), &self.pkey)
                .map_err(wrap_openssl_err)?,
            "rs512" => openssl::sign::Signer::new(MessageDigest::sha512(), &self.pkey)
                .map_err(wrap_openssl_err)?,
            _ => return Err(Error::UnsupportedType),
        };

        let signed_data = signer.sign_oneshot_to_vec(data)?;

        // println!("sig: {}", Hexlify(&signed_data));

        Ok(signed_data)
    }

    fn reserve_size(&self) -> usize {
        1024 + self.certs_size + self.timestamp_size + self.ocsp_size.get() // the Cose_Sign1 contains complete certs, timestamps and ocsp so account for size
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        let mut certs: Vec<Vec<u8>> = Vec::new();

        for c in &self.signcerts {
            let cert = c.to_der().map_err(wrap_openssl_err)?;
            certs.push(cert);
        }

        Ok(certs)
    }

    fn alg(&self) -> Option<String> {
        Some(self.alg.to_owned())
    }

    fn time_authority_url(&self) -> Option<String> {
        self.tsa_url.clone()
    }

    fn ocsp_val(&self) -> Option<Vec<u8>> {
        // update OCSP if needed
        self.update_ocsp();

        let ocsp_data = self.ocsp_rsp.take();
        let ocsp_rsp = ocsp_data.ocsp_der.clone();
        self.ocsp_rsp.set(ocsp_data);
        if !ocsp_rsp.is_empty() {
            Some(ocsp_rsp)
        } else {
            None
        }
    }
}

fn wrap_io_err(err: std::io::Error) -> Error {
    Error::IoError(err)
}

fn wrap_openssl_err(err: openssl::error::ErrorStack) -> Error {
    Error::OpenSslError(err)
}

#[allow(unused_imports)]
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    use tempfile::tempdir;

    use crate::{openssl::temp_signer::get_temp_signer, Signer};

    #[test]
    fn signer_from_files() {
        let temp_dir = tempdir().unwrap();

        let (signer, _) = get_temp_signer(&temp_dir.path());
        let data = b"some sample content to sign";

        let signature = signer.sign(data).unwrap();
        println!("signature len = {}", signature.len());
        assert!(signature.len() <= signer.reserve_size());
    }

    #[test]
    fn sign_ps256() {
        let cert_bytes = include_bytes!("../../tests/fixtures/temp_cert.data");
        let key_bytes = include_bytes!("../../tests/fixtures/temp_priv_key.data");

        let signer =
            RsaSigner::from_signcert_and_pkey(cert_bytes, key_bytes, "ps256".to_string(), None)
                .unwrap();

        let data = b"some sample content to sign";

        let signature = signer.sign(data).unwrap();
        println!("signature len = {}", signature.len());
        assert!(signature.len() <= signer.reserve_size());
    }

    #[test]
    fn sign_rs256() {
        let cert_bytes = include_bytes!("../../tests/fixtures/temp_cert.data");
        let key_bytes = include_bytes!("../../tests/fixtures/temp_priv_key.data");

        let signer =
            RsaSigner::from_signcert_and_pkey(cert_bytes, key_bytes, "rs256".to_string(), None)
                .unwrap();

        let data = b"some sample content to sign";

        let signature = signer.sign(data).unwrap();
        println!("signature len = {}", signature.len());
        assert!(signature.len() <= signer.reserve_size());
    }
}
