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

use crate::error::{Error, Result};
use crate::status_tracker::{log_item, StatusTracker};
use crate::time_stamp::gt_to_datetime;
use crate::validation_status;
#[cfg(not(target_arch = "wasm32"))]
use crate::validator::get_validator;
#[cfg(not(target_arch = "wasm32"))]
use crate::validator::CoseValidator;
use crate::validator::ValidationInfo;

#[cfg(target_arch = "wasm32")]
use crate::wasm::webcrypto_validator::validate_async;

use crate::asn1::rfc3161::TstInfo;
use ciborium::value::Value;
use conv::*;
use coset::{sig_structure_data, Label, TaggedCborSerializable};

use std::str::FromStr;

use x509_parser::der_parser::ber::parse_ber_sequence;
use x509_parser::der_parser::oid;
use x509_parser::oid_registry::Oid;
use x509_parser::prelude::*;

const RSA_OID: Oid<'static> = oid!(1.2.840 .113549 .1 .1 .1);
const EC_PUBLICKEY_OID: Oid<'static> = oid!(1.2.840 .10045 .2 .1);
const ECDSA_WITH_SHA256_OID: Oid<'static> = oid!(1.2.840 .10045 .4 .3 .2);
const ECDSA_WITH_SHA384_OID: Oid<'static> = oid!(1.2.840 .10045 .4 .3 .3);
const ECDSA_WITH_SHA512_OID: Oid<'static> = oid!(1.2.840 .10045 .4 .3 .4);
const RSASSA_PSS_OID: Oid<'static> = oid!(1.2.840 .113549 .1 .1 .10);
const SHA256_WITH_RSAENCRYPTION_OID: Oid<'static> = oid!(1.2.840 .113549 .1 .1 .11);
const SHA384_WITH_RSAENCRYPTION_OID: Oid<'static> = oid!(1.2.840 .113549 .1 .1 .12);
const SHA512_WITH_RSAENCRYPTION_OID: Oid<'static> = oid!(1.2.840 .113549 .1 .1 .13);
const ED25519_OID: Oid<'static> = oid!(1.3.101 .112);
const SHA256_OID: Oid<'static> = oid!(2.16.840 .1 .101 .3 .4 .2 .1);
const SHA384_OID: Oid<'static> = oid!(2.16.840 .1 .101 .3 .4 .2 .2);
const SHA512_OID: Oid<'static> = oid!(2.16.840 .1 .101 .3 .4 .2 .3);
const SECP521R1_OID: Oid<'static> = oid!(1.3.132 .0 .35);
const SECP384R1_OID: Oid<'static> = oid!(1.3.132 .0 .34);
const PRIME256V1_OID: Oid<'static> = oid!(1.2.840 .10045 .3 .1 .7);

/********************** Supported Valiators ***************************************
    RS256	RSASSA-PKCS1-v1_5 using SHA-256 - not recommended
    RS384	RSASSA-PKCS1-v1_5 using SHA-384 - not recommended
    RS512	RSASSA-PKCS1-v1_5 using SHA-512 - not recommended
    PS256	RSASSA-PSS using SHA-256 and MGF1 with SHA-256
    PS384	RSASSA-PSS using SHA-384 and MGF1 with SHA-384
    PS512	RSASSA-PSS using SHA-512 and MGF1 with SHA-512
    ES256	ECDSA using P-256 and SHA-256
    ES384	ECDSA using P-384 and SHA-384
    ES512	ECDSA using P-521 and SHA-512
    ED25519 Edwards Curve 25519
**********************************************************************************/

fn get_cose_sign1(
    cose_bytes: &[u8],
    data: &[u8],
    validation_log: &mut impl StatusTracker,
) -> Result<coset::CoseSign1> {
    match <coset::CoseSign1 as TaggedCborSerializable>::from_tagged_slice(cose_bytes) {
        Ok(mut sign1) => {
            sign1.payload = Some(data.to_vec()); // restore payload for verification check

            Ok(sign1)
        }
        Err(coset_error) => {
            let log_item = log_item!(
                "Cose_Sign1",
                "could not deserialize signature",
                "get_cose_sign1"
            )
            .error(Error::InvalidCoseSignature { coset_error })
            .validation_status(validation_status::CLAIM_SIGNATURE_MISMATCH);

            validation_log.log_silent(log_item);

            Err(Error::CoseSignature)
        }
    }
}
fn check_cert(
    _alg: &str,
    ca_der_bytes: &[u8],
    validation_log: &mut impl StatusTracker,
    _tst_info_opt: Option<&TstInfo>,
) -> Result<()> {
    // get the cert in der format
    let (_rem, signcert) = X509Certificate::from_der(ca_der_bytes).map_err(|_err| {
        let log_item = log_item!(
            "Cose_Sign1",
            "certificate could not be parsed",
            "check_cert_alg"
        )
        .error(Error::CoseInvalidCert)
        .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
        validation_log.log_silent(log_item);
        Error::CoseInvalidCert
    })?;

    // cert version must be 3
    if signcert.version() != X509Version::V3 {
        let log_item = log_item!(
            "Cose_Sign1",
            "certificate version incorrect",
            "check_cert_alg"
        )
        .error(Error::CoseInvalidCert)
        .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
        validation_log.log_silent(log_item);

        return Err(Error::CoseInvalidCert);
    }

    // check for cert expiration
    if let Some(tst_info) = _tst_info_opt {
        // was there a time stamp associtation with this signature, is verify against that time
        let signing_time = gt_to_datetime(tst_info.gen_time.clone());
        if !signcert
            .validity()
            .is_valid_at(x509_parser::time::ASN1Time::from_timestamp(
                signing_time.timestamp(),
            ))
        {
            let log_item = log_item!("Cose_Sign1", "certificate expired", "check_cert_alg")
                .error(Error::CoseCertExpiration)
                .validation_status(validation_status::SIGNING_CREDENTIAL_EXPIRED);
            validation_log.log_silent(log_item);

            return Err(Error::CoseCertExpiration);
        }
    } else {
        // no timestamp so check against current time
        // use instant to avoid wasm issues
        let now_f64 = instant::now() / 1000.0;
        let now: i64 = now_f64
            .approx_as::<i64>()
            .map_err(|_e| Error::BadParam("system time invalid".to_string()))?;

        if !signcert
            .validity()
            .is_valid_at(x509_parser::time::ASN1Time::from_timestamp(now))
        {
            let log_item = log_item!("Cose_Sign1", "certificate expired", "check_cert_alg")
                .error(Error::CoseCertExpiration)
                .validation_status(validation_status::SIGNING_CREDENTIAL_EXPIRED);
            validation_log.log_silent(log_item);

            return Err(Error::CoseCertExpiration);
        }
    }

    let cert_alg = signcert.signature_algorithm.algorithm.clone();

    // check algorithm needed from cert

    // cert must be signed with one the following algorithm
    if !(cert_alg == SHA256_WITH_RSAENCRYPTION_OID
        || cert_alg == SHA384_WITH_RSAENCRYPTION_OID
        || cert_alg == SHA512_WITH_RSAENCRYPTION_OID
        || cert_alg == ECDSA_WITH_SHA256_OID
        || cert_alg == ECDSA_WITH_SHA384_OID
        || cert_alg == ECDSA_WITH_SHA512_OID
        || cert_alg == RSASSA_PSS_OID
        || cert_alg == ED25519_OID)
    {
        let log_item = log_item!(
            "Cose_Sign1",
            "certificate algorithm not supported",
            "check_cert_alg"
        )
        .error(Error::CoseInvalidCert)
        .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
        validation_log.log_silent(log_item);

        return Err(Error::CoseInvalidCert);
    }

    // verify rsassa_pss parameters
    if cert_alg == RSASSA_PSS_OID {
        if let Some(parameters) = &signcert.signature_algorithm.parameters {
            let seq = parameters
                .as_sequence()
                .map_err(|_err| Error::CoseInvalidCert)?;
            if seq.len() < 3 {
                let log_item = log_item!(
                    "Cose_Sign1",
                    "certificate incorrect rsapss algorithm",
                    "check_cert_alg"
                )
                .error(Error::CoseInvalidCert)
                .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
                validation_log.log_silent(log_item);

                return Err(Error::CoseInvalidCert);
            }

            // get hash algorithm
            let (_b, ha_alg) = AlgorithmIdentifier::from_der(
                seq[0]
                    .content
                    .as_slice()
                    .map_err(|_err| Error::CoseInvalidCert)?,
            )
            .map_err(|_err| Error::CoseInvalidCert)?;

            let (_b, mgf_ai) = AlgorithmIdentifier::from_der(
                seq[1]
                    .content
                    .as_slice()
                    .map_err(|_err| Error::CoseInvalidCert)?,
            )
            .map_err(|_err| Error::CoseInvalidCert)?;

            let mgf_ai_parameters = mgf_ai.parameters.ok_or(Error::CoseInvalidCert)?;
            let s = mgf_ai_parameters
                .as_sequence()
                .map_err(|_err| Error::CoseInvalidCert)?;
            let t0 = &s[0];
            //let _t1 = &s[1];
            let mfg_ai_params_algorithm = t0.as_oid_val().map_err(|_err| Error::CoseInvalidCert)?;

            // must be the same
            if ha_alg.algorithm != mfg_ai_params_algorithm {
                let log_item = log_item!(
                    "Cose_Sign1",
                    "certificate algorithm error",
                    "check_cert_alg"
                )
                .error(Error::CoseInvalidCert)
                .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
                validation_log.log_silent(log_item);

                return Err(Error::CoseInvalidCert);
            }

            // check for one of the mandatory types
            if !(ha_alg.algorithm == SHA256_OID
                || ha_alg.algorithm == SHA384_OID
                || ha_alg.algorithm == SHA512_OID)
            {
                let log_item = log_item!(
                    "Cose_Sign1",
                    "certificate hash algorithm not supported",
                    "check_cert_alg"
                )
                .error(Error::CoseInvalidCert)
                .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
                validation_log.log_silent(log_item);

                return Err(Error::CoseInvalidCert);
            }
        } else {
            let log_item = log_item!(
                "Cose_Sign1",
                "certificate missing algorithm parameters",
                "check_cert_alg"
            )
            .error(Error::CoseInvalidCert)
            .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
            validation_log.log_silent(log_item);

            return Err(Error::CoseInvalidCert);
        }
    }

    // check curves for SPKI EC algorithms
    let pk = signcert.public_key();
    let skpi_alg = &pk.algorithm;

    if skpi_alg.algorithm == EC_PUBLICKEY_OID {
        if let Some(parameters) = &skpi_alg.parameters {
            let named_curve_oid = parameters
                .as_oid_val()
                .map_err(|_err| Error::CoseInvalidCert)?;

            // must be one of these named curves
            if !(named_curve_oid == PRIME256V1_OID
                || named_curve_oid == SECP384R1_OID
                || named_curve_oid == SECP521R1_OID)
            {
                let log_item = log_item!(
                    "Cose_Sign1",
                    "certificate unsupported EC curve",
                    "check_cert_alg"
                )
                .error(Error::CoseInvalidCert)
                .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
                validation_log.log_silent(log_item);

                return Err(Error::CoseInvalidCert);
            }
        } else {
            return Err(Error::CoseInvalidCert);
        }
    }

    // check modulus minumum length (for RSA & PSS algorithms)
    if skpi_alg.algorithm == RSA_OID || skpi_alg.algorithm == RSASSA_PSS_OID {
        let (_, skpi_ber) = parse_ber_sequence(pk.subject_public_key.data)
            .map_err(|_err| Error::CoseInvalidCert)?;

        let seq = skpi_ber
            .as_sequence()
            .map_err(|_err| Error::CoseInvalidCert)?;
        if seq.len() < 2 {
            return Err(Error::CoseInvalidCert);
        }

        let modulus = seq[0].as_bigint().ok_or(Error::CoseInvalidCert)?;

        if modulus.bits() < 2048 {
            let log_item = log_item!(
                "Cose_Sign1",
                "certificate key length too short",
                "check_cert_alg"
            )
            .error(Error::CoseInvalidCert)
            .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
            validation_log.log_silent(log_item);

            return Err(Error::CoseInvalidCert);
        }
    }

    // check cert values
    let tbscert = &signcert.tbs_certificate;

    let is_self_signed = tbscert.is_ca() && tbscert.issuer_uid == tbscert.subject_uid;

    // only allowable for self sigbed
    if !is_self_signed && tbscert.issuer_uid.is_some() || tbscert.subject_uid.is_some() {
        let log_item = log_item!(
            "Cose_Sign1",
            "certificate issuer and subject cannot be the same",
            "check_cert_alg"
        )
        .error(Error::CoseInvalidCert)
        .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
        validation_log.log_silent(log_item);

        return Err(Error::CoseInvalidCert);
    }

    // non self signed CA certs are not allowed, must be an end entity (leaf) cert
    if tbscert.is_ca() && !is_self_signed {
        return Err(Error::CoseInvalidCert);
    }

    let mut aki_good = false;
    let mut ski_good = false;
    let mut key_usage_good = false;
    let mut handled_all_critical = true;
    let extended_key_usage_good = match tbscert.extended_key_usage() {
        Some((_critical, eku)) => {
            if eku.any {
                let log_item = log_item!(
                    "Cose_Sign1",
                    "certificate 'any' EKU not allowed",
                    "check_cert_alg"
                )
                .error(Error::CoseInvalidCert)
                .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
                validation_log.log_silent(log_item);

                return Err(Error::CoseInvalidCert);
            }

            if !(eku.email_protection || eku.ocsp_signing || eku.time_stamping) {
                let log_item = log_item!(
                    "Cose_Sign1",
                    "certificate missing required EKU",
                    "check_cert_alg"
                )
                .error(Error::CoseInvalidCert)
                .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
                validation_log.log_silent(log_item);

                return Err(Error::CoseInvalidCert);
            }

            // one or the other || either of these two, and no others field
            if (eku.ocsp_signing && eku.time_stamping)
                || ((eku.ocsp_signing ^ eku.time_stamping)
                    && (eku.client_auth
                        | eku.code_signing
                        | eku.email_protection
                        | eku.server_auth))
            {
                let log_item = log_item!(
                    "Cose_Sign1",
                    "certificate invalid set of EKUs",
                    "check_cert_alg"
                )
                .error(Error::CoseInvalidCert)
                .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
                validation_log.log_silent(log_item);

                return Err(Error::CoseInvalidCert);
            }

            true
        }
        None => tbscert.is_ca(), // if is not ca it must be present
    };

    // popluate needed extension info
    for e in signcert.extensions() {
        match e.parsed_extension() {
            ParsedExtension::AuthorityKeyIdentifier(_aki) => {
                aki_good = true;
            }
            ParsedExtension::SubjectKeyIdentifier(_spki) => {
                ski_good = true;
            }
            ParsedExtension::KeyUsage(ku) => {
                if ku.digital_signature() {
                    if ku.key_cert_sign() && !tbscert.is_ca() {
                        let log_item = log_item!(
                            "Cose_Sign1",
                            "certificate missing digitalSignature EKU",
                            "check_cert_alg"
                        )
                        .error(Error::CoseInvalidCert)
                        .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
                        validation_log.log_silent(log_item);

                        return Err(Error::CoseInvalidCert);
                    }
                    key_usage_good = true;
                }
                if ku.key_cert_sign() {
                    key_usage_good = true;
                }
                // todo: warn if not marked critical
                // if !e.critical { // warn here somehow}
            }
            ParsedExtension::CertificatePolicies(_) => (),
            ParsedExtension::PolicyMappings(_) => (),
            ParsedExtension::SubjectAlternativeName(_) => (),
            ParsedExtension::BasicConstraints(_) => (),
            ParsedExtension::NameConstraints(_) => (),
            ParsedExtension::PolicyConstraints(_) => (),
            ParsedExtension::ExtendedKeyUsage(_) => (),
            ParsedExtension::CRLDistributionPoints(_) => (),
            ParsedExtension::InhibitAnyPolicy(_) => (),
            ParsedExtension::AuthorityInfoAccess(_) => (),
            ParsedExtension::NSCertType(_) => (),
            ParsedExtension::CRLNumber(_) => (),
            ParsedExtension::ReasonCode(_) => (),
            ParsedExtension::InvalidityDate(_) => (),
            ParsedExtension::Unparsed => {
                if e.critical {
                    // unhandled critical extension
                    handled_all_critical = false;
                }
            }
            _ => {
                if e.critical {
                    // unhandled critical extension
                    handled_all_critical = false;
                }
            }
        }
    }

    // if cert is a CA must have valid SubjectKeyIdentifier
    ski_good = if tbscert.is_ca() { ski_good } else { true };

    // check all flags
    if aki_good && ski_good && key_usage_good && extended_key_usage_good && handled_all_critical {
        Ok(())
    } else {
        let log_item = log_item!(
            "Cose_Sign1",
            "certificate params incorrect",
            "check_cert_alg"
        )
        .error(Error::CoseInvalidCert)
        .validation_status(validation_status::SIGNING_CREDENTIAL_INVALID);
        validation_log.log_silent(log_item);

        Err(Error::CoseInvalidCert)
    }
}

pub(crate) fn get_validator_str(cs1: &coset::CoseSign1) -> Result<String> {
    // find the supported handler for the algorithm
    let validator_str = match cs1.protected.header.alg {
        Some(ref alg) => {
            let alg_str = match alg {
                coset::RegisteredLabelWithPrivate::PrivateUse(a) => match a {
                    -39 => "ps512",
                    -38 => "ps384",
                    -37 => "ps256",
                    -36 => "es512",
                    -35 => "es384",
                    -7 => "es256",
                    // todo: deprecated  figure out lecacy support for RS signatures
                    -259 => "rs512",
                    -258 => "rs384",
                    -257 => "rs256",

                    -8 => "ed25519",
                    _ => "unknown",
                },
                coset::RegisteredLabelWithPrivate::Assigned(a) => match a {
                    coset::iana::Algorithm::PS512 => "ps512",
                    coset::iana::Algorithm::PS384 => "ps384",
                    coset::iana::Algorithm::PS256 => "ps256",
                    coset::iana::Algorithm::ES512 => "es512",
                    coset::iana::Algorithm::ES384 => "es384",
                    coset::iana::Algorithm::ES256 => "es256",
                    // todo: deprecated  figure out lecacy support for RS signatures
                    coset::iana::Algorithm::RS512 => "rs512",
                    coset::iana::Algorithm::RS384 => "rs384",
                    coset::iana::Algorithm::RS256 => "rs256",
                    coset::iana::Algorithm::EdDSA => "ed25519",
                    _ => "unknown",
                },
                coset::RegisteredLabelWithPrivate::Text(a) => a,
            };

            Some(alg_str.to_owned())
        }
        None => None,
    }
    .ok_or(Error::CoseSignatureAlgorithmNotSupported)?;

    Ok(validator_str)
}

fn get_sign_cert(sign1: &coset::CoseSign1) -> Result<Vec<u8>> {
    // element 0 is the signing cert
    let certs = get_sign_certs(sign1)?;
    Ok(certs[0].clone())
}
// get the public key der
fn get_sign_certs(sign1: &coset::CoseSign1) -> Result<Vec<Vec<u8>>> {
    let mut certs: Vec<Vec<u8>> = Vec::new();

    // get the public key der
    if let Some(der) = sign1
        .unprotected
        .rest
        .iter()
        .find_map(|x: &(Label, Value)| {
            if x.0 == Label::Text("x5chain".to_string()) {
                Some(x.1.clone())
            } else {
                None
            }
        })
    {
        match der {
            Value::Array(cert_chain) => {
                // handle array of certs
                for c in cert_chain {
                    if let Value::Bytes(der_bytes) = c {
                        certs.push(der_bytes.clone());
                    }
                }
                Ok(certs)
            }
            Value::Bytes(ref der_bytes) => {
                // handle single cert case
                certs.push(der_bytes.clone());
                Ok(certs)
            }
            _ => Err(Error::CoseX5ChainMissing),
        }
    } else {
        Err(Error::CoseX5ChainMissing)
    }
}

// Note: this function is only used to get the display string and not for cert validation.
fn get_signing_time(
    sign1: &coset::CoseSign1,
    data: &[u8],
    validation_log: &mut impl StatusTracker,
) -> Option<chrono::DateTime<chrono::Utc>> {
    // get timestamp info if available

    if let Ok(tst_info) = get_timestamp_info(sign1, data) {
        Some(gt_to_datetime(tst_info.gen_time))
    } else if let Some(t) = &sign1
        .unprotected
        .rest
        .iter()
        .find_map(|x: &(Label, Value)| {
            if x.0 == Label::Text("temp_signing_time".to_string()) {
                Some(x.1.clone())
            } else {
                None
            }
        })
    {
        let time_cbor = serde_cbor::to_vec(t).ok()?;
        let dt_string: String = serde_cbor::from_slice(&time_cbor).ok()?;
        chrono::DateTime::<chrono::Utc>::from_str(&dt_string).ok()
    } else {
        let log_item = log_item!(
            "Cose_Sign1",
            "invalid timestamp message imprint",
            "get_signing_time"
        )
        .error(Error::CoseTimeStampMismatch)
        .validation_status(validation_status::TIMESTAMP_MISMATCH);
        validation_log
            .log(log_item, Some(Error::CoseTimeStampMismatch))
            .ok()?;

        None
    }
}

// return appropriate TstInfo if available
fn get_timestamp_info(sign1: &coset::CoseSign1, data: &[u8]) -> Result<TstInfo> {
    // parse the temp timestamp
    if let Some(t) = &sign1
        .unprotected
        .rest
        .iter()
        .find_map(|x: &(Label, Value)| {
            if x.0 == Label::Text("sigTst".to_string()) {
                Some(x.1.clone())
            } else {
                None
            }
        })
    {
        let alg = get_validator_str(sign1)?;
        let time_cbor = serde_cbor::to_vec(t)?;
        let tst_infos = crate::time_stamp::cose_sigtst_to_tstinfos(&time_cbor, data, &alg)?;

        // there should only be one but consider handling more in the future since it is technically ok
        if !tst_infos.is_empty() {
            return Ok(tst_infos[0].clone());
        }
    }
    Err(Error::NotFound)
}

fn extract_subject_from_cert(cert: &X509Certificate) -> Result<String> {
    cert.subject()
        .iter_organization()
        .map(|attr| attr.as_str())
        .last()
        .ok_or(Error::CoseX5ChainMissing)?
        .map(|attr| attr.to_string())
        .map_err(|_e| Error::CoseX5ChainMissing)
}

/// Asynchronously validate a COSE_SIGN1 byte vector and verify against expected data
/// cose_bytes - byte array containing the raw COSE_SIGN1 data
/// data:  data that was used to create the cose_bytes, these must match
/// addition_data: additional optional data that may have been used during signing
/// returns - Ok on success
pub async fn verify_cose_async(
    cose_bytes: Vec<u8>,
    data: Vec<u8>,
    additional_data: Vec<u8>,
    signature_only: bool,
    validation_log: &mut impl StatusTracker,
) -> Result<ValidationInfo> {
    let mut sign1 = get_cose_sign1(&cose_bytes, &data, validation_log)?;

    let validator_str = match get_validator_str(&sign1) {
        Ok(s) => s,
        Err(_) => {
            let log_item = log_item!(
                "Cose_Sign1",
                "unsupported or missing Cose algorithhm",
                "verify_cose_async"
            )
            .error(Error::CoseSignatureAlgorithmNotSupported)
            .validation_status(validation_status::ALGORITHM_UNSUPPORTED);
            validation_log.log(log_item, Some(Error::CoseSignatureAlgorithmNotSupported))?;

            // one of these must exist
            return Err(Error::CoseSignatureAlgorithmNotSupported);
        }
    };

    // build result structure
    let mut result = ValidationInfo::default();

    // get the public key der
    let der_bytes = get_sign_cert(&sign1)?;

    // verify cert matches requested algorithm
    if !signature_only {
        // verify certs
        match get_timestamp_info(&sign1, &data) {
            Ok(tst_info) => {
                check_cert(&validator_str, &der_bytes, validation_log, Some(&tst_info))?
            }
            Err(e) => {
                // log timestamp errors
                match e {
                    Error::NotFound => {
                        check_cert(&validator_str, &der_bytes, validation_log, None)?
                    }
                    Error::CoseTimeStampMismatch => {
                        let log_item = log_item!(
                            "Cose_Sign1",
                            "timestamp message imprint did not match",
                            "verify_cose"
                        )
                        .error(Error::CoseTimeStampMismatch)
                        .validation_status(validation_status::TIMESTAMP_MISMATCH);
                        validation_log.log(log_item, Some(Error::CoseTimeStampMismatch))?;
                    }
                    Error::CoseTimeStampValidity => {
                        let log_item =
                            log_item!("Cose_Sign1", "timestamp outside of validity", "verify_cose")
                                .error(Error::CoseTimeStampValidity)
                                .validation_status(validation_status::TIMESTAMP_OUTSIDE_VALIDITY);
                        validation_log.log(log_item, Some(Error::CoseTimeStampValidity))?;
                    }
                    _ => {
                        let log_item =
                            log_item!("Cose_Sign1", "error parsing timestamp", "verify_cose")
                                .error(Error::CoseInvalidTimeStamp);
                        validation_log.log(log_item, Some(Error::CoseInvalidTimeStamp))?;

                        return Err(Error::CoseInvalidTimeStamp);
                    }
                }
            }
        }
    }

    // Check the signature, which needs to have the same `additional_data` provided, by
    // providing a closure that can do the verify operation.
    sign1.payload = Some(data.clone()); // restore payload

    let p_header = sign1.protected.clone();

    let tbs = sig_structure_data(
        coset::SignatureContext::CoseSign1,
        p_header,
        None,
        &additional_data,
        sign1.payload.as_ref().unwrap_or(&vec![]),
    ); // get "to be signed" bytes

    if let Ok(issuer) =
        validate_with_cert_async(&validator_str, &sign1.signature, &tbs, &der_bytes).await
    {
        result.issuer_org = Some(issuer);
        result.validated = true;
        result.alg = validator_str.to_owned();

        // parse the temp time for now util we have TA
        result.date = get_signing_time(&sign1, &data, validation_log);
    }

    Ok(result)
}

pub fn get_signing_info(
    cose_bytes: &[u8],
    data: &[u8],
    validation_log: &mut impl StatusTracker,
) -> ValidationInfo {
    let mut date = None;
    let mut issuer_org = None;
    let mut alg = "".to_string();

    let _ = get_cose_sign1(cose_bytes, data, validation_log).and_then(|sign1| {
        // get the public key der
        let der_bytes = get_sign_cert(&sign1)?;

        let _ = X509Certificate::from_der(&der_bytes).map(|(_rem, signcert)| {
            date = get_signing_time(&sign1, data, validation_log);
            issuer_org = extract_subject_from_cert(&signcert).ok();
            if let Ok(a) = get_validator_str(&sign1) {
                alg = a;
            }

            (_rem, signcert)
        });

        Ok(sign1)
    });

    ValidationInfo {
        issuer_org,
        date,
        alg,
        validated: false,
    }
}

/// Validate a COSE_SIGN1 byte vector and verify against expected data
/// cose_bytes - byte array containing the raw COSE_SIGN1 data
/// data:  data that was used to create the cose_bytes, these must match
/// addition_data: additional optional data that may have been used during signing
/// returns - Ok on success
#[cfg(not(target_arch = "wasm32"))]
pub fn verify_cose(
    cose_bytes: &[u8],
    data: &[u8],
    additional_data: &[u8],
    signature_only: bool,
    validation_log: &mut impl StatusTracker,
) -> Result<ValidationInfo> {
    let sign1 = get_cose_sign1(cose_bytes, data, validation_log)?;

    let validator_str = match get_validator_str(&sign1) {
        Ok(s) => s,
        Err(_) => {
            let log_item = log_item!(
                "Cose_Sign1",
                "unsupported or missing Cose algorithhm",
                "verify_cose"
            )
            .error(Error::CoseSignatureAlgorithmNotSupported)
            .validation_status(validation_status::ALGORITHM_UNSUPPORTED);

            validation_log.log(log_item, Some(Error::CoseSignatureAlgorithmNotSupported))?;

            return Err(Error::CoseSignatureAlgorithmNotSupported);
        }
    };

    let validator =
        get_validator(&validator_str).ok_or(Error::CoseSignatureAlgorithmNotSupported)?;

    // build result structure
    let mut result = ValidationInfo::default();

    // get the cert chain
    let certs = get_sign_certs(&sign1)?;

    // get the public key der
    let der_bytes = &certs[0];

    if !signature_only {
        // verify certs
        match get_timestamp_info(&sign1, data) {
            Ok(tst_info) => check_cert(&validator_str, der_bytes, validation_log, Some(&tst_info))?,
            Err(e) => {
                // log timestamp errors
                match e {
                    Error::NotFound => check_cert(&validator_str, der_bytes, validation_log, None)?,
                    Error::CoseTimeStampMismatch => {
                        let log_item = log_item!(
                            "Cose_Sign1",
                            "timestamp message imprint did not match",
                            "verify_cose"
                        )
                        .error(Error::CoseTimeStampMismatch)
                        .validation_status(validation_status::TIMESTAMP_MISMATCH);
                        validation_log.log(log_item, Some(Error::CoseTimeStampMismatch))?;
                    }
                    Error::CoseTimeStampValidity => {
                        let log_item =
                            log_item!("Cose_Sign1", "timestamp outside of validity", "verify_cose")
                                .error(Error::CoseTimeStampValidity)
                                .validation_status(validation_status::TIMESTAMP_OUTSIDE_VALIDITY);
                        validation_log.log(log_item, Some(Error::CoseTimeStampValidity))?;
                    }
                    _ => {
                        let log_item =
                            log_item!("Cose_Sign1", "error parsing timestamp", "verify_cose")
                                .error(Error::CoseInvalidTimeStamp);
                        validation_log.log(log_item, Some(Error::CoseInvalidTimeStamp))?;

                        return Err(Error::CoseInvalidTimeStamp);
                    }
                }
            }
        }
    }

    // Check the signature, which needs to have the same `additional_data` provided, by
    // providing a closure that can do the verify operation.
    sign1.verify_signature(additional_data, |sig, verify_data| -> Result<()> {
        if let Ok(issuer) = validate_with_cert(validator, sig, verify_data, der_bytes) {
            result.issuer_org = Some(issuer);
            result.validated = true;
            result.alg = validator_str.to_string();

            // parse the temp time for now util we have TA
            result.date = get_signing_time(&sign1, data, validation_log);
        }
        // Note: not adding validation_log entry here since caller will supply claim specific info to log
        Ok(())
    })?;

    Ok(result)
}

#[cfg(target_arch = "wasm32")]
pub fn verify_cose(
    _cose_bytes: &[u8],
    _data: &[u8],
    _additional_data: &[u8],
    _signature_only: bool,
    _validation_log: &mut impl StatusTracker,
) -> Result<ValidationInfo> {
    Err(Error::CoseVerifier)
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_with_cert(
    validator: Box<dyn CoseValidator>,
    sig: &[u8],
    data: &[u8],
    der_bytes: &[u8],
) -> Result<String> {
    // get the cert in der format
    let (_rem, signcert) =
        X509Certificate::from_der(der_bytes).map_err(|_err| Error::CoseInvalidCert)?;
    let pk = signcert.public_key();
    let pk_der = pk.raw;

    if validator.validate(sig, data, pk_der)? {
        Ok(extract_subject_from_cert(&signcert)?)
    } else {
        Err(Error::CoseSignature)
    }
}

#[cfg(target_arch = "wasm32")]
async fn validate_with_cert_async(
    validator_str: &str,
    sig: &[u8],
    data: &[u8],
    der_bytes: &[u8],
) -> Result<String> {
    let (_rem, signcert) =
        X509Certificate::from_der(der_bytes).map_err(|_err| Error::CoseMissingKey)?;
    let pk = signcert.public_key();
    let pk_der = pk.raw;

    if validate_async(validator_str, sig, data, pk_der).await? {
        Ok(extract_subject_from_cert(&signcert)?)
    } else {
        Err(Error::CoseSignature)
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn validate_with_cert_async(
    _validator_str: &str,
    _sig: &[u8],
    _data: &[u8],
    _der_bytes: &[u8],
) -> Result<String> {
    Err(Error::CoseSignatureAlgorithmNotSupported)
}
#[allow(unused_imports)]
#[cfg(feature = "file_io")]
#[cfg(test)]
pub mod tests {
    #![allow(clippy::unwrap_used)]

    use sha2::digest::generic_array::sequence::Shorten;

    use crate::status_tracker::DetailedStatusTracker;

    use super::*;

    #[test]
    #[cfg(feature = "file_io")]
    fn test_expired_cert() {
        let mut validation_log = DetailedStatusTracker::new();

        let mut cert_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        cert_path.push("tests/fixtures/rsa-pss256_key-expired.pub");

        let expired_cert = std::fs::read(&cert_path).unwrap();

        if let Ok(signcert) = openssl::x509::X509::from_pem(&expired_cert) {
            let der_bytes = signcert.to_der().unwrap();
            assert!(check_cert("ps256", &der_bytes, &mut validation_log, None).is_err());

            assert!(!validation_log.get_log().is_empty());

            assert_eq!(
                validation_log.get_log()[0].validation_status,
                Some(validation_status::SIGNING_CREDENTIAL_EXPIRED.to_string())
            );
        }
    }

    #[test]
    fn test_verify_cose_good() {
        let validator = get_validator("ps256").unwrap();

        let sig_bytes = include_bytes!("../tests/fixtures/sig.data");
        let data_bytes = include_bytes!("../tests/fixtures/data.data");
        let key_bytes = include_bytes!("../tests/fixtures/key.data");

        assert!(validator
            .validate(sig_bytes, data_bytes, key_bytes)
            .unwrap());
    }

    #[test]
    fn test_verify_ec_good() {
        // EC signatures
        let mut validator = get_validator("es384").unwrap();

        let sig_es384_bytes = include_bytes!("../tests/fixtures/sig_es384.data");
        let data_es384_bytes = include_bytes!("../tests/fixtures/data_es384.data");
        let key_es384_bytes = include_bytes!("../tests/fixtures/key_es384.data");

        assert!(validator
            .validate(sig_es384_bytes, data_es384_bytes, key_es384_bytes)
            .unwrap());

        validator = get_validator("es512").unwrap();

        let sig_es512_bytes = include_bytes!("../tests/fixtures/sig_es512.data");
        let data_es512_bytes = include_bytes!("../tests/fixtures/data_es512.data");
        let key_es512_bytes = include_bytes!("../tests/fixtures/key_es512.data");

        assert!(validator
            .validate(sig_es512_bytes, data_es512_bytes, key_es512_bytes)
            .unwrap());
    }

    #[test]
    fn test_verify_cose_bad() {
        let validator = get_validator("ps256").unwrap();

        let sig_bytes = include_bytes!("../tests/fixtures/sig.data");
        let data_bytes = include_bytes!("../tests/fixtures/data.data");
        let key_bytes = include_bytes!("../tests/fixtures/key.data");

        let mut bad_bytes = data_bytes.to_vec();
        bad_bytes[0] = b'c';
        bad_bytes[1] = b'2';
        bad_bytes[2] = b'p';
        bad_bytes[3] = b'a';

        assert!(!validator
            .validate(sig_bytes, &bad_bytes, key_bytes)
            .unwrap());
    }

    #[test]
    #[cfg(feature = "file_io")]
    fn test_cert_algorithms() {
        use tempfile::tempdir;

        use crate::openssl::temp_signer;

        let mut validation_log = DetailedStatusTracker::new();

        let temp_dir = tempdir().unwrap();
        let (_, cert_path) = temp_signer::get_ec_signer(&temp_dir.path(), "es256", None);
        let es256_cert = std::fs::read(&cert_path).unwrap();

        let (_, cert_path) = temp_signer::get_ec_signer(&temp_dir.path(), "es384", None);
        let es384_cert = std::fs::read(&cert_path).unwrap();

        let (_, cert_path) = temp_signer::get_ec_signer(&temp_dir.path(), "es512", None);
        let es512_cert = std::fs::read(&cert_path).unwrap();

        let (_, cert_path) = temp_signer::get_rsa_signer(&temp_dir.path(), "ps256", None);
        let rsa_pss256_cert = std::fs::read(&cert_path).unwrap();

        if let Ok(signcert) = openssl::x509::X509::from_pem(&es256_cert) {
            let der_bytes = signcert.to_der().unwrap();
            assert!(check_cert("es256", &der_bytes, &mut validation_log, None).is_ok());
        }

        if let Ok(signcert) = openssl::x509::X509::from_pem(&es384_cert) {
            let der_bytes = signcert.to_der().unwrap();
            assert!(check_cert("es384", &der_bytes, &mut validation_log, None).is_ok());
        }

        if let Ok(signcert) = openssl::x509::X509::from_pem(&es512_cert) {
            let der_bytes = signcert.to_der().unwrap();
            assert!(check_cert("es512", &der_bytes, &mut validation_log, None).is_ok());
        }

        if let Ok(signcert) = openssl::x509::X509::from_pem(&rsa_pss256_cert) {
            let der_bytes = signcert.to_der().unwrap();
            assert!(check_cert("ps256", &der_bytes, &mut validation_log, None).is_ok());
        }
    }
}
