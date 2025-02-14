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

use crate::{validator::CoseValidator, Error, Result};
use openssl::ec::EcKey;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;

pub struct EcValidator {
    alg: String,
}

impl EcValidator {
    pub fn new(alg: &str) -> Self {
        EcValidator {
            alg: alg.to_owned(),
        }
    }
}

impl CoseValidator for EcValidator {
    fn validate(&self, sig: &[u8], data: &[u8], pkey: &[u8]) -> Result<bool> {
        let public_key = EcKey::public_key_from_der(pkey).map_err(|_err| Error::CoseSignature)?;
        let key = PKey::from_ec_key(public_key).map_err(wrap_openssl_err)?;

        let mut verifier = match self.alg.as_ref() {
            "es256" => openssl::sign::Verifier::new(MessageDigest::sha256(), &key)?,
            "es384" => openssl::sign::Verifier::new(MessageDigest::sha384(), &key)?,
            "es512" => openssl::sign::Verifier::new(MessageDigest::sha512(), &key)?,
            _ => return Err(Error::UnsupportedType),
        };

        // is this an expected P1363 sig size
        if sig.len()
            != match self.alg.as_ref() {
                "es256" => 64,
                "es384" => 96,
                "es512" => 132,
                _ => return Err(Error::UnsupportedType),
            }
        {
            return Err(Error::CoseSignature);
        }

        // convert P1363 sig to DER sig
        let sig_len = sig.len() / 2;
        let r = openssl::bn::BigNum::from_slice(&sig[0..sig_len])
            .map_err(|_err| Error::CoseSignature)?;
        let s = openssl::bn::BigNum::from_slice(&sig[sig_len..])
            .map_err(|_err| Error::CoseSignature)?;

        let ecdsa_sig = openssl::ecdsa::EcdsaSig::from_private_components(r, s)
            .map_err(|_err| Error::CoseSignature)?;
        let sig_der = ecdsa_sig.to_der().map_err(|_err| Error::CoseSignature)?;

        verifier.update(data).map_err(wrap_openssl_err)?;
        verifier
            .verify(&sig_der)
            .map_err(|_err| Error::CoseSignature)
    }
}

fn wrap_openssl_err(err: openssl::error::ErrorStack) -> Error {
    Error::OpenSslError(err)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    use tempfile::tempdir;

    use crate::{
        openssl::{ec_signer::EcSigner, temp_signer},
        signer::ConfigurableSigner,
        utils::test::fixture_path,
        Signer,
    };

    #[test]
    fn sign_and_validate_es256() {
        let temp_dir = tempdir().unwrap();

        let (signer, cert_path) = temp_signer::get_ec_signer(&temp_dir.path(), "es256", None);

        let data = b"some sample content to sign";
        println!("data len = {}", data.len());

        let signature = signer.sign(data).unwrap();
        println!("signature.len = {}", signature.len());
        assert!(signature.len() >= 64);
        assert!(signature.len() <= signer.reserve_size());

        let cert_bytes = std::fs::read(&cert_path).unwrap();

        let signcert = openssl::x509::X509::from_pem(&cert_bytes).unwrap();
        let pub_key = signcert.public_key().unwrap().public_key_to_der().unwrap();

        let validator = EcValidator::new("es256");
        assert!(validator.validate(&signature, data, &pub_key).unwrap());
    }

    #[test]
    fn sign_and_validate_es384() {
        let temp_dir = tempdir().unwrap();

        let (signer, cert_path) = temp_signer::get_ec_signer(&temp_dir.path(), "es384", None);

        let data = b"some sample content to sign";
        println!("data len = {}", data.len());

        let signature = signer.sign(data).unwrap();
        println!("signature.len = {}", signature.len());
        assert!(signature.len() >= 64);
        assert!(signature.len() <= signer.reserve_size());

        let cert_bytes = std::fs::read(&cert_path).unwrap();

        let signcert = openssl::x509::X509::from_pem(&cert_bytes).unwrap();
        let pub_key = signcert.public_key().unwrap().public_key_to_der().unwrap();

        let validator = EcValidator::new("es384");
        assert!(validator.validate(&signature, data, &pub_key).unwrap());
    }

    #[test]
    fn sign_and_validate_es512() {
        let temp_dir = tempdir().unwrap();

        let (signer, cert_path) = temp_signer::get_ec_signer(&temp_dir.path(), "es512", None);

        let data = b"some sample content to sign";
        println!("data len = {}", data.len());

        let signature = signer.sign(data).unwrap();
        println!("signature.len = {}", signature.len());
        assert!(signature.len() >= 64);
        assert!(signature.len() <= signer.reserve_size());

        let cert_bytes = std::fs::read(&cert_path).unwrap();

        let signcert = openssl::x509::X509::from_pem(&cert_bytes).unwrap();
        let pub_key = signcert.public_key().unwrap().public_key_to_der().unwrap();

        let validator = EcValidator::new("es512");
        assert!(validator.validate(&signature, data, &pub_key).unwrap());
    }

    #[test]
    fn bad_sig_es256() {
        let temp_dir = tempdir().unwrap();

        let (signer, cert_path) = temp_signer::get_ec_signer(&temp_dir.path(), "es256", None);

        let data = b"some sample content to sign";
        println!("data len = {}", data.len());
        let mut signature = signer.sign(data).unwrap();

        signature.push(10);

        let cert_bytes = std::fs::read(&cert_path).unwrap();
        let signcert = openssl::x509::X509::from_pem(&cert_bytes).unwrap();
        let pub_key = signcert.public_key().unwrap().public_key_to_der().unwrap();

        let validator = EcValidator::new("es256");
        let validated = validator.validate(&signature, data, &pub_key);
        assert!(validated.is_err());
    }

    #[test]
    fn bad_data_es256() {
        let temp_dir = tempdir().unwrap();

        let (signer, cert_path) = temp_signer::get_ec_signer(&temp_dir.path(), "es256", None);

        let mut data = b"some sample content to sign".to_vec();
        println!("data len = {}", data.len());
        let signature = signer.sign(&data).unwrap();

        data[5] = 10;
        data[6] = 11;

        let cert_bytes = std::fs::read(&cert_path).unwrap();
        let signcert = openssl::x509::X509::from_pem(&cert_bytes).unwrap();
        let pub_key = signcert.public_key().unwrap().public_key_to_der().unwrap();

        let validator = EcValidator::new("es256");
        assert!(!validator.validate(&signature, &data, &pub_key).unwrap());
    }

    #[test]
    fn sign_and_validate_with_chain() {
        let pkey_path = fixture_path("bob.key");
        let cert_path = fixture_path("bob.pem");

        let signer =
            EcSigner::from_files(&cert_path, &pkey_path, "es256".to_string(), None).unwrap();

        let data = b"some sample content to sign";
        println!("data len = {}", data.len());

        let signature = signer.sign(data).unwrap();
        println!("signature.len = {}", signature.len());
        assert!(signature.len() >= 64);
        assert!(signature.len() <= signer.reserve_size());

        let cert_bytes = &signer.certs().unwrap()[0];
        let signcert = openssl::x509::X509::from_der(cert_bytes).unwrap();

        let pub_key = signcert.public_key().unwrap().public_key_to_der().unwrap();
        let validator = EcValidator::new("es256");
        assert!(validator.validate(&signature, data, &pub_key).unwrap());
    }
}
