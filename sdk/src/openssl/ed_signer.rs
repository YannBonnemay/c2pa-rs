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

use std::{fs, path::Path};

use crate::{signer::ConfigurableSigner, Error, Result, Signer};

use openssl::{
    pkey::{PKey, Private},
    x509::X509,
};

use super::check_chain_order;

/// Implements `Signer` trait using OpenSSL's implementation of
/// Edwards Curve encryption.
pub struct EdSigner {
    signcerts: Vec<X509>,
    pkey: PKey<Private>,

    certs_size: usize,
    timestamp_size: usize,

    alg: String,
    tsa_url: Option<String>,
}

impl ConfigurableSigner for EdSigner {
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
        let certs_size = signcert.len();
        let signcerts = X509::stack_from_pem(signcert).map_err(wrap_openssl_err)?;
        let pkey = PKey::private_key_from_pem(pkey).map_err(wrap_openssl_err)?;

        if alg.to_lowercase() != "ed25519" {
            return Err(Error::UnsupportedType); // only ed25519 is supported by C2PA
        }

        // make sure cert chains are in order
        if !check_chain_order(&signcerts) {
            return Err(Error::BadParam(
                "certificate chain is not in correct order".to_string(),
            ));
        }

        Ok(EdSigner {
            signcerts,
            pkey,
            certs_size,
            timestamp_size: 4096, // todo: call out to TSA to get actual timestamp and use that size
            alg: "ed25519".to_string(),
            tsa_url,
        })
    }
}

impl Signer for EdSigner {
    fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut signer =
            openssl::sign::Signer::new_without_digest(&self.pkey).map_err(wrap_openssl_err)?;

        let signed_data = signer.sign_oneshot_to_vec(data)?;

        Ok(signed_data)
    }

    fn alg(&self) -> Option<String> {
        Some(self.alg.to_owned())
    }

    fn certs(&self) -> Result<Vec<Vec<u8>>> {
        let mut certs: Vec<Vec<u8>> = Vec::new();

        for c in &self.signcerts {
            let cert = c.to_der().map_err(wrap_openssl_err)?;
            certs.push(cert);
        }

        Ok(certs)
    }

    fn time_authority_url(&self) -> Option<String> {
        self.tsa_url.clone()
    }

    fn reserve_size(&self) -> usize {
        1024 + self.certs_size + self.timestamp_size // the Cose_Sign1 contains complete certs and timestamps so account for size
    }
}

fn wrap_io_err(err: std::io::Error) -> Error {
    Error::IoError(err)
}

fn wrap_openssl_err(err: openssl::error::ErrorStack) -> Error {
    Error::OpenSslError(err)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    use tempfile::tempdir;

    use crate::openssl::temp_signer;

    #[test]
    fn ed25519_signer() {
        let temp_dir = tempdir().unwrap();

        let (signer, _) = temp_signer::get_ed_signer(&temp_dir.path(), "ed25519", None);

        let data = b"some sample content to sign";
        println!("data len = {}", data.len());

        let signature = signer.sign(data).unwrap();
        println!("signature.len = {}", signature.len());
        assert!(signature.len() >= 64);
        assert!(signature.len() <= signer.reserve_size());
    }
}
