use base64::Engine;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::key::Signer;

#[derive(Serialize)]
struct Header<'a> {
    alg: &'a str,
    typ: &'a str,
}

#[derive(Serialize)]
struct Claims<'a> {
    iat: u64,
    exp: u64,
    iss: &'a str,
}

pub fn build(signer: &Signer, issuer: &str, now: u64) -> Result<String> {
    if issuer.is_empty() {
        return Err(Error::message("JWT issuer must not be empty"));
    }
    let header = serde_json::to_vec(&Header {
        alg: "RS256",
        typ: "JWT",
    })
    .map_err(|e| Error::message(format!("could not encode JWT header: {e}")))?;
    let claims = serde_json::to_vec(&Claims {
        iat: now.saturating_sub(60),
        exp: now + 540,
        iss: issuer,
    })
    .map_err(|e| Error::message(format!("could not encode JWT claims: {e}")))?;

    let header = b64url(&header);
    let claims = b64url(&claims);
    let signing_input = format!("{header}.{claims}");
    let signature = signer.sign(signing_input.as_bytes())?;
    Ok(format!("{signing_input}.{}", b64url(&signature)))
}

pub fn unix_now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|e| Error::message(format!("system clock error: {e}")))
}

fn b64url(value: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use serde_json::Value;

    #[test]
    fn jwt_has_expected_shape() {
        let mut rng = rand::thread_rng();
        let key = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
        let signer = Signer::from_pem(pem.as_bytes()).unwrap();
        let jwt = build(&signer, "123456", 1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        for part in &parts {
            assert!(!part.contains('='));
        }

        let decode = |value: &str| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(value)
                .unwrap()
        };
        let header: Value = serde_json::from_slice(&decode(parts[0])).unwrap();
        let claims: Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(claims["iss"], "123456");
        assert_eq!(
            claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap(),
            600
        );
        assert_eq!(decode(parts[2]).len(), 256);
    }
}
